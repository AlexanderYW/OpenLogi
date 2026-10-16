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

/// One key to paint, on the panel `spec` describes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Paint<'a> {
    spec: &'a KeyDisplaySpec,
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

/// What the watcher believes is on every panel it has painted, and which
/// panels are refusing writes — the sans-I/O half of the watcher. Only
/// [`Self::record`] adds a painted face, and only for a write the device
/// acknowledged.
#[derive(Debug, Default)]
struct Painted {
    panels: Vec<PaintedPanel>,
    failing: Vec<DeviceRoute>,
}

impl Painted {
    /// The keys `specs` wants that are not known to be showing their face.
    ///
    /// Forgets a panel that left `specs` (it may come back showing anything)
    /// and one whose generation moved (it may have reset), so both are
    /// repainted in full.
    ///
    /// Also forgets that a panel which left `specs` was failing: one that
    /// comes back is new.
    fn plan<'a>(&mut self, specs: &'a [KeyDisplaySpec]) -> Vec<Paint<'a>> {
        self.panels.retain(|panel| {
            specs
                .iter()
                .any(|spec| spec.route == panel.route && spec.generation == panel.generation)
        });
        self.failing
            .retain(|failing| specs.iter().any(|spec| &spec.route == failing));
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
                    .map(move |(key, face)| Paint {
                        spec,
                        key: *key,
                        face: *face,
                    })
            })
            .collect()
    }

    /// Note that the device acknowledged `paint`, so its panel is not
    /// failing.
    fn record(&mut self, paint: &Paint) {
        let spec = paint.spec;
        self.failing.retain(|failing| failing != &spec.route);
        match self
            .panels
            .iter_mut()
            .find(|panel| panel.route == spec.route)
        {
            Some(panel) if panel.generation == spec.generation => {
                panel.faces.insert(paint.key, paint.face);
            }
            // A newer generation was planned meanwhile: this paint predates a
            // reset, so it proves nothing about the panel now.
            Some(_) => {}
            None => self.panels.push(PaintedPanel {
                route: spec.route.clone(),
                generation: spec.generation,
                faces: BTreeMap::from([(paint.key, paint.face)]),
            }),
        }
    }

    /// Note a failed write on `route`; `true` when the panel was not already
    /// failing, so the failure is news worth a `warn`.
    fn failed(&mut self, route: &DeviceRoute) -> bool {
        if self.failing.contains(route) {
            return false;
        }
        self.failing.push(route.clone());
        true
    }

    /// Whether any panel's last write failed and should be retried.
    fn is_failing(&self) -> bool {
        !self.failing.is_empty()
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

    /// The tile for `face`, or a blank one when `face` cannot be rendered —
    /// so the key never keeps showing the glyph of an action it is no longer
    /// bound to. `None` only when not even a blank tile renders.
    ///
    /// A render failure is a defect in the artwork, not the device: rendering
    /// again cannot fix it, so the blank stands in for `face` until the
    /// binding changes.
    fn get_or_blank(&mut self, face: KeyFace, size: u16) -> Option<Arc<KeyImageJpeg>> {
        match self.get(face, size) {
            Ok(tile) => Some(tile),
            Err(error) => {
                warn!(%error, ?face, "key image could not be rendered — showing it blank");
                self.get(KeyFace::Blank, size)
                    .inspect_err(|error| warn!(%error, "blank key image could not be rendered"))
                    .ok()
            }
        }
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
        tokio::select! {
            () = paint_all(&pending, &mut painted, &mut tiles, &access) => {}
            _ = &mut shutdown => return ManagerCompletion::Graceful,
        }
        let retry = painted.is_failing().then(|| Instant::now() + RETRY_DELAY);
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

/// Paint every key in `pending`, recording each acknowledged one and each
/// keypad whose write failed.
///
/// The first failed write on a keypad skips that keypad's remaining keys: a
/// device that did not answer one tile will not answer the next, and each
/// would wait out its own acknowledgement timeout.
async fn paint_all(
    pending: &[Paint<'_>],
    painted: &mut Painted,
    tiles: &mut Tiles,
    access: &DeviceAccess,
) {
    let mut failed: Vec<&DeviceRoute> = Vec::new();
    for paint in pending {
        let route = &paint.spec.route;
        if failed.contains(&route) {
            continue;
        }
        let Some(tile) = tiles.get_or_blank(paint.face, paint.spec.display.key_size) else {
            continue;
        };
        let (display, key) = (paint.spec.display, paint.key);
        let result = access
            .op(route)
            .run(HidppOperation::WriteKeyImage, |channel| async move {
                write_key_image_on(&channel, display, key, &tile).await
            })
            .await;
        match result {
            Ok(()) => painted.record(paint),
            Err(error) => {
                if painted.failed(route) {
                    warn!(%error, %route, key = ?paint.key, "key image write failed — retrying");
                } else {
                    debug!(%error, %route, key = ?paint.key, "key image write failed again — retrying");
                }
                failed.push(route);
            }
        }
    }
}

#[cfg(test)]
mod tests;
