//! Device brightness: the `0x8040` BrightnessControl's read-back and write echo.
//!
//! Two facts, one authority each, like Fn-lock. The *setting* is
//! `config.toml`'s per-device `brightness`, which the agent re-applies when a
//! device that forgets its level (the MX Creative Keypad reports itself
//! transient) reconnects. The *reading* is the device's own: a read answers
//! with the level plus the range the device itself advertised, and a write is
//! answered by the state the device echoes, so the slider shows what the
//! device accepted rather than what was requested — firmware is allowed to
//! clamp. Until the first read lands there is nothing to show but a loading
//! state: inventing a range would invent a device.

use openlogi_core::hid::{BrightnessState, WriteError};
use tracing::warn;

use super::device_key::DeviceKey;
use super::events::StateEvents;
use super::load::BrightnessLoad;
use super::{AppState, StateEvent};

impl AppState {
    /// Whether the active device reports a BrightnessControl (`0x8040`).
    #[must_use]
    pub fn current_brightness_supported(&self) -> bool {
        self.current_record()
            .and_then(|record| record.capabilities)
            .is_some_and(|capabilities| capabilities.brightness)
    }

    /// What is known of the active device's own brightness.
    #[must_use]
    pub fn current_brightness_load(&self) -> BrightnessLoad {
        self.current_record()
            .and_then(|record| self.pointer.reads.brightness_load(&record.device_key()))
            .cloned()
            .unwrap_or_default()
    }

    /// Persist `level` for the active device and write it now through the
    /// agent. The slider rests on the requested value while only its range is
    /// the device's own; the device's echo arrives as
    /// [`Self::apply_brightness_written`] and shows the level that actually
    /// landed. No-op when no device is selected or it reports no brightness.
    pub fn commit_brightness(&mut self, level: u16) -> StateEvents {
        let events = self.for_current_device(StateEvent::BrightnessChanged);
        if !self.current_brightness_supported() {
            return events;
        }
        let Some(record) = self.current_record() else {
            return events;
        };
        let device_key = record.device_key();
        let persistent_key = record.persistent_config_key().map(str::to_string);
        let route = record.route.clone();
        if let Some(persistent_key) = persistent_key {
            self.config
                .edit(|config| config.set_brightness(&persistent_key, level));
            if !self.persist_and_reload("brightness") {
                return events;
            }
        }
        let Some(route) = route else {
            return events;
        };
        // Keep the range the device reported and move only the level; the
        // echo below corrects both if firmware clamped anything.
        if let BrightnessLoad::Ready(state) = self.current_brightness_load() {
            self.pointer.reads.set_brightness_ready(
                &device_key,
                BrightnessState {
                    level,
                    min: state.min,
                    max: state.max,
                    steps: state.steps,
                },
            );
        }
        self.send_ipc(crate::services::ipc::SetBrightness {
            route,
            level,
            key: device_key,
        });
        events
    }

    /// The device's answer to a [`Self::commit_brightness`] write: show the
    /// echoed state, or — when the device refused or could not be reached —
    /// re-read it, so an optimistic thumb does not stand in for the device.
    pub fn apply_brightness_written(
        &mut self,
        key: &DeviceKey,
        result: Result<BrightnessState, WriteError>,
    ) -> StateEvents {
        match result {
            Ok(state) => self.pointer.reads.set_brightness_ready(key, state),
            Err(error) => {
                warn!(%error, key = %key, "brightness write did not land — re-reading the device");
                self.pointer.reads.refresh_brightness(key);
            }
        }
        StateEvent::BrightnessChanged(key.clone()).into()
    }

    /// Retry a failed brightness read by revalidating the same subscription.
    pub(crate) fn retry_brightness_read(&mut self, key: &DeviceKey) -> StateEvents {
        self.pointer.reads.refresh_brightness(key);
        StateEvent::BrightnessChanged(key.clone()).into()
    }

    pub(super) fn load_current_brightness(&mut self, cx: &mut gpui::Context<Self>) {
        if !self.current_brightness_supported() {
            return;
        }
        let Some((key, route)) = self
            .current_record()
            .and_then(|record| Some((record.device_key(), record.route.clone()?)))
        else {
            return;
        };
        self.pointer
            .reads
            .ensure_brightness(key, route, self.ipc_sender(), cx);
    }
}
