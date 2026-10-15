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
    /// The lowest level the device accepts.
    pub min: u16,
    /// The highest level the device accepts.
    pub max: u16,
    /// The discrete level count the device advertises, or `0`/`1` when it
    /// takes any level between `min` and `max`. A GUI slider bounds itself by
    /// `min`/`max` and may use this only to decide whether intermediate
    /// values must snap.
    pub steps: u16,
}
