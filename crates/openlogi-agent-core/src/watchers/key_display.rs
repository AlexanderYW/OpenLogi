//! Keypad LCD keys: keep each key's image in step with its binding.
//!
//! The orchestrator publishes one [`KeyDisplaySpec`] per online keypad whose
//! keys are displays — what every key should show, derived from the same
//! effective bindings the keyboard capture session dispatches. This watcher
//! owns the device side: it remembers what it last painted on each panel,
//! renders and writes only the keys that differ, and repaints a whole panel
//! when the spec's re-arm generation moves (a reconnect or a system wake,
//! after which the device shows its own default again).
//!
//! The images are volatile panel state with nothing to restore: a stop
//! simply abandons any write in flight.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use openlogi_core::binding::GridKey;
use openlogi_hid::{DeviceRoute, HidppOperation, KeyDisplay, KeyImageJpeg, write_key_image_on};
use tokio::sync::{oneshot, watch};
use tokio::time::Instant;
use tracing::{debug, warn};

use super::retry::{RETRY_DELAY, wait_for_deadline};
use super::shutdown::{ManagerCompletion, WatcherHandle};
use crate::hardware::DeviceAccess;

pub mod render;

pub use render::KeyFace;
use render::{RenderError, render_tile};

/// What one keypad's LCD keys should show.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyDisplaySpec {
    /// The keypad.
    pub route: DeviceRoute,
    /// Where its keys sit on its panel.
    pub display: KeyDisplay,
    /// The capture re-arm generation: it moves when a device reconnects or
    /// the system wakes, and a moved generation means the panel must be
    /// assumed to have lost what was painted on it.
    pub generation: u64,
    /// Every key's face.
    pub faces: BTreeMap<GridKey, KeyFace>,
}

/// Read-only, coalescing view of the published key-display specs.
pub type SharedKeyDisplaySpecs = watch::Receiver<Arc<Vec<KeyDisplaySpec>>>;

/// One key to paint.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Paint {
    route: DeviceRoute,
    display: KeyDisplay,
    generation: u64,
    key: GridKey,
    face: KeyFace,
}

/// What the watcher believes is on one panel.
#[derive(Debug)]
struct PaintedPanel {
    route: DeviceRoute,
    generation: u64,
    faces: BTreeMap<GridKey, KeyFace>,
}

/// What the watcher believes is on every panel it has painted — the
/// sans-I/O half of the watcher. Only [`Self::record`] adds to it, and only
/// for a write the device acknowledged.
#[derive(Debug, Default)]
struct Painted {
    panels: Vec<PaintedPanel>,
}

impl Painted {
    /// The keys `specs` wants that are not known to be showing their face.
    ///
    /// Forgets a panel that left `specs` (it may come back showing anything)
    /// and one whose generation moved (it may have reset), so both are
    /// repainted in full.
    fn plan(&mut self, specs: &[KeyDisplaySpec]) -> Vec<Paint> {
        self.panels.retain(|panel| {
            specs
                .iter()
                .any(|spec| spec.route == panel.route && spec.generation == panel.generation)
        });
        specs
            .iter()
            .flat_map(|spec| {
                let painted = self
                    .panels
                    .iter()
                    .find(|panel| panel.route == spec.route)
                    .map(|panel| &panel.faces);
                spec.faces
                    .iter()
                    .filter(move |(key, face)| {
                        painted.and_then(|faces| faces.get(key)) != Some(face)
                    })
                    .map(|(key, face)| Paint {
                        route: spec.route.clone(),
                        display: spec.display,
                        generation: spec.generation,
                        key: *key,
                        face: *face,
                    })
            })
            .collect()
    }

    /// Note that the device acknowledged `paint`.
    fn record(&mut self, paint: &Paint) {
        match self
            .panels
            .iter_mut()
            .find(|panel| panel.route == paint.route)
        {
            Some(panel) if panel.generation == paint.generation => {
                panel.faces.insert(paint.key, paint.face);
            }
            // A newer generation was planned meanwhile: this paint predates a
            // reset, so it proves nothing about the panel now.
            Some(_) => {}
            None => self.panels.push(PaintedPanel {
                route: paint.route.clone(),
                generation: paint.generation,
                faces: BTreeMap::from([(paint.key, paint.face)]),
            }),
        }
    }
}

/// Rendered tiles, by face and tile size: the same icon on two keys, or
/// again after a rebind, is encoded once.
#[derive(Default)]
struct Tiles {
    rendered: HashMap<(KeyFace, u16), Arc<KeyImageJpeg>>,
}

impl Tiles {
    fn get(&mut self, face: KeyFace, size: u16) -> Result<Arc<KeyImageJpeg>, RenderError> {
        if let Some(tile) = self.rendered.get(&(face, size)) {
            return Ok(Arc::clone(tile));
        }
        let tile = Arc::new(render_tile(face, size)?);
        self.rendered.insert((face, size), Arc::clone(&tile));
        Ok(tile)
    }
}

/// Spawn the key-display manager thread.
#[must_use]
pub fn spawn(specs: &SharedKeyDisplaySpecs, access: DeviceAccess) -> WatcherHandle {
    let specs = specs.clone();
    WatcherHandle::spawn("openlogi-key-display-watcher", move |shutdown| {
        manage(specs, access, shutdown)
    })
}

async fn manage(
    mut specs: SharedKeyDisplaySpecs,
    access: DeviceAccess,
    mut shutdown: oneshot::Receiver<()>,
) -> ManagerCompletion {
    let mut painted = Painted::default();
    let mut tiles = Tiles::default();
    loop {
        let wanted = Arc::clone(&specs.borrow_and_update());
        let pending = painted.plan(&wanted);
        let retry = tokio::select! {
            failed = paint_all(&pending, &mut painted, &mut tiles, &access) => {
                failed.then(|| Instant::now() + RETRY_DELAY)
            }
            _ = &mut shutdown => return ManagerCompletion::Graceful,
        };
        tokio::select! {
            changed = specs.changed() => {
                if changed.is_err() {
                    return ManagerCompletion::Unexpected;
                }
            }
            () = wait_for_deadline(retry) => {}
            _ = &mut shutdown => return ManagerCompletion::Graceful,
        }
    }
}

/// Paint every key in `pending`, recording each acknowledged one. Returns
/// whether any write failed and should be retried.
///
/// The first failed write on a keypad skips that keypad's remaining keys: a
/// device that did not answer one tile will not answer the next, and each
/// would wait out its own acknowledgement timeout.
async fn paint_all(
    pending: &[Paint],
    painted: &mut Painted,
    tiles: &mut Tiles,
    access: &DeviceAccess,
) -> bool {
    let mut failed: Vec<&DeviceRoute> = Vec::new();
    for paint in pending {
        if failed.contains(&&paint.route) {
            continue;
        }
        let tile = match tiles.get(paint.face, paint.display.key_size) {
            Ok(tile) => tile,
            Err(error) => {
                // A defect in the artwork, not the device: retrying cannot
                // fix it, so leave this key and paint the rest.
                warn!(%error, key = ?paint.key, face = ?paint.face, "key image could not be rendered");
                continue;
            }
        };
        let (display, key) = (paint.display, paint.key);
        let result = access
            .op(&paint.route)
            .run(HidppOperation::WriteKeyImage, |channel| async move {
                write_key_image_on(&channel, display, key, &tile).await
            })
            .await;
        match result {
            Ok(()) => painted.record(paint),
            Err(error) => {
                debug!(%error, route = %paint.route, key = ?paint.key, "key image write failed — retrying");
                failed.push(&paint.route);
            }
        }
    }
    !failed.is_empty()
}

#[cfg(test)]
mod tests;
