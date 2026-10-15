//! Known Logitech keypad identities.
//!
//! The devices speak ordinary HID++ and are handled by the generic device
//! layer. They are listed because some of them report no (or an unmodelled)
//! `DeviceType` on feature `0x0005`, so the protocol cannot name a kind — and
//! kind decides the capture plane. This table is the direct-path sibling of
//! the receiver pairing register: a per-product identity fact claimed here,
//! which `openlogi-device` converts to a `DeviceKind` at its own boundary.
//!
//! An entry also carries its LCD key panel's geometry, the one hardware fact
//! no protocol feature reports: the panel is driven over a vendor report, so
//! only the product identity says it exists and where its keys sit.

use crate::LOGITECH_VENDOR_ID;

/// A known Logitech keypad identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeypadDescriptor {
    /// USB vendor ID.
    pub vendor_id: u16,
    /// USB product ID.
    pub product_id: u16,
    /// Marketed product name.
    pub name: &'static str,
    /// The LCD panel behind the key grid, when the keys are displays.
    pub key_display: Option<KeyDisplay>,
}

impl KeypadDescriptor {
    const fn logitech(product_id: u16, name: &'static str, key_display: KeyDisplay) -> Self {
        Self {
            vendor_id: LOGITECH_VENDOR_ID,
            product_id,
            name,
            key_display: Some(key_display),
        }
    }
}

/// Where a keypad's square LCD keys sit on its panel, in panel pixels.
///
/// Keys are laid out on a regular grid: key `(row, col)` (zero-based from the
/// top-left) is the `key_size`-square tile whose top-left corner is
/// `origin + (col, row) · pitch`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyDisplay {
    /// Side length of one key's tile.
    pub key_size: u16,
    /// Top-left corner of the top-left key, as `(x, y)`.
    pub origin: (u16, u16),
    /// Distance between the top-left corners of adjacent keys.
    pub pitch: u16,
    /// Rows and columns of keys.
    pub grid: (u16, u16),
}

impl KeyDisplay {
    /// The top-left corner of key `(row, col)`, or `None` off the grid.
    #[must_use]
    pub const fn key_origin(&self, row: u16, col: u16) -> Option<(u16, u16)> {
        if row >= self.grid.0 || col >= self.grid.1 {
            return None;
        }
        Some((
            self.origin.0 + col * self.pitch,
            self.origin.1 + row * self.pitch,
        ))
    }
}

/// The MX Creative Keypad's 480×480 panel: nine 118-pixel keys, 40 pixels
/// apart. Verified on hardware 2026-10 (`openlogi-hid`'s
/// `mx_keypad_lcd_probe`): solid tiles written at these positions filled
/// each key edge to edge, in row-major order.
const MX_CREATIVE_KEYPAD_DISPLAY: KeyDisplay = KeyDisplay {
    key_size: 118,
    origin: (23, 6),
    pitch: 158,
    grid: (3, 3),
};

/// All standalone keypad identities OpenLogi recognizes.
pub const KEYPADS: &[KeypadDescriptor] = &[
    // MX Creative Keypad — wired USB-C only, HID++ 4.5. Its `0x0005` marketing
    // name reads correctly, but its DeviceType is `None`, so this entry is the
    // only kind source for it (see `openlogi-device`'s direct-path resolve).
    KeypadDescriptor::logitech(0xc354, "MX Creative Keypad", MX_CREATIVE_KEYPAD_DISPLAY),
];

/// Finds a keypad descriptor by its USB identity.
#[must_use]
pub fn find_keypad(vendor_id: u16, product_id: u16) -> Option<&'static KeypadDescriptor> {
    KEYPADS
        .iter()
        .find(|device| device.vendor_id == vendor_id && device.product_id == product_id)
}

#[cfg(test)]
mod tests {
    use super::{KEYPADS, KeyDisplay, find_keypad};
    use crate::LOGITECH_VENDOR_ID;

    #[test]
    fn keypad_identities_are_unique() {
        for (index, device) in KEYPADS.iter().enumerate() {
            assert!(
                KEYPADS[..index]
                    .iter()
                    .all(|other| (other.vendor_id, other.product_id)
                        != (device.vendor_id, device.product_id)),
                "duplicate keypad identity {:04x}:{:04x}",
                device.vendor_id,
                device.product_id
            );
        }
    }

    #[test]
    fn complete_identity_selects_mx_creative_keypad_metadata() {
        let keypad = find_keypad(LOGITECH_VENDOR_ID, 0xc354).expect("MX Creative Keypad identity");

        assert_eq!(keypad.name, "MX Creative Keypad");
    }

    #[test]
    fn mx_creative_keypad_keys_sit_on_the_verified_panel_grid() {
        let display = find_keypad(LOGITECH_VENDOR_ID, 0xc354)
            .and_then(|keypad| keypad.key_display)
            .expect("the MX Creative Keypad's keys are displays");

        assert_eq!(display.key_origin(0, 0), Some((23, 6)));
        assert_eq!(display.key_origin(0, 2), Some((339, 6)));
        assert_eq!(display.key_origin(2, 1), Some((181, 322)));
        // The last key still fits the 480-pixel panel.
        let (x, y) = display.key_origin(2, 2).expect("bottom-right key");
        assert!(x + display.key_size <= 480 && y + display.key_size <= 480);
    }

    #[test]
    fn key_origin_is_none_off_the_grid() {
        let display = KeyDisplay {
            key_size: 10,
            origin: (0, 0),
            pitch: 12,
            grid: (2, 3),
        };
        assert_eq!(display.key_origin(1, 2), Some((24, 12)));
        assert!(display.key_origin(2, 0).is_none());
        assert!(display.key_origin(0, 3).is_none());
    }

    #[test]
    fn lookup_requires_the_matching_vendor_and_product() {
        assert!(find_keypad(0xffff, 0xc354).is_none());
        assert!(find_keypad(LOGITECH_VENDOR_ID, 0xc355).is_none());
    }
}
