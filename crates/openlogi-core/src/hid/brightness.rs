//! Brightness read-back state — pure data, no I/O.
//!
//! The HID++ `0x8040 BrightnessControl` reads and writes that produce this
//! state remain in `openlogi-device`.

use serde::{Deserialize, Serialize};

/// What a device's BrightnessControl reports: the level it is at now and the
/// range it advertised.
///
/// Crosses the agent↔GUI IPC, so field order is wire format — changes require
/// a `PROTOCOL_VERSION` bump (guarded by `openlogi-ipc/tests/wire_format.rs`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BrightnessState {
    /// The level the device reports currently being set.
    pub level: u16,
    /// The lowest level the device advertises. It may be `0`, which OpenLogi
    /// never writes — see [`WritableBrightness`].
    pub min: u16,
    /// The highest level the device accepts.
    pub max: u16,
    /// The discrete level count the device advertises, or `0`/`1` when it
    /// takes any level between `min` and `max`. A GUI slider bounds itself by
    /// [`BrightnessState::writable`], not `min`/`max`, and may use this only
    /// to decide whether intermediate values must snap.
    pub steps: u16,
}

/// The lowest level OpenLogi writes, whatever the device advertises: the
/// MX Creative Keypad advertises a minimum of `0`, but a write of `0` resets
/// the device rather than dimming it.
const LOWEST_WRITABLE_LEVEL: u16 = 1;

/// The levels OpenLogi may write to one device's BrightnessControl: the
/// range the device advertised, raised so it never includes `0`.
///
/// This is the single owner of that decision — the device layer clamps every
/// write through [`Self::clamp`] and a GUI slider bounds itself by
/// [`Self::min`]/[`Self::max`], so neither re-derives the floor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WritableBrightness {
    min: u16,
    max: u16,
}

impl WritableBrightness {
    /// The writable levels of a device that advertised `min..=max`. A device
    /// whose advertised maximum is below the floor is pinned to the floor.
    #[must_use]
    pub fn new(advertised_min: u16, advertised_max: u16) -> Self {
        let min = advertised_min.max(LOWEST_WRITABLE_LEVEL);
        Self {
            min,
            max: advertised_max.max(min),
        }
    }

    /// The lowest level OpenLogi writes to this device.
    #[must_use]
    pub fn min(self) -> u16 {
        self.min
    }

    /// The highest level OpenLogi writes to this device.
    #[must_use]
    pub fn max(self) -> u16 {
        self.max
    }

    /// `level` moved into the writable range.
    #[must_use]
    pub fn clamp(self, level: u16) -> u16 {
        level.clamp(self.min, self.max)
    }
}

impl BrightnessState {
    /// The levels OpenLogi may write to the device that reported this state.
    #[must_use]
    pub fn writable(&self) -> WritableBrightness {
        WritableBrightness::new(self.min, self.max)
    }
}

#[cfg(test)]
mod tests {
    use super::{BrightnessState, WritableBrightness};

    #[test]
    fn writable_range_never_includes_zero() {
        // The MX Creative Keypad's advertised range.
        let keypad = WritableBrightness::new(0, 100);
        assert_eq!((keypad.min(), keypad.max()), (1, 100));
        assert_eq!(keypad.clamp(0), 1);
        assert_eq!(keypad.clamp(1), 1);
        assert_eq!(keypad.clamp(55), 55);
        assert_eq!(keypad.clamp(u16::MAX), 100);
    }

    #[test]
    fn writable_range_keeps_an_advertised_floor_above_one() {
        let range = WritableBrightness::new(10, 1000);
        assert_eq!((range.min(), range.max()), (10, 1000));
        assert_eq!(range.clamp(3), 10);
    }

    #[test]
    fn writable_range_pins_a_degenerate_device_to_the_floor() {
        let range = WritableBrightness::new(0, 0);
        assert_eq!((range.min(), range.max()), (1, 1));
        assert_eq!(range.clamp(0), 1);
    }

    #[test]
    fn state_reports_the_writable_range_of_its_advertised_one() {
        let state = BrightnessState {
            level: 0,
            min: 0,
            max: 100,
            steps: 101,
        };
        assert_eq!(state.writable(), WritableBrightness::new(0, 100));
    }
}
