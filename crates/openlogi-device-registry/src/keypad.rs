//! Known Logitech keypad identities.
//!
//! Unlike the Litra and gaming-headset tables, these entries carry no driver
//! metadata: the devices speak ordinary HID++ and are handled by the generic
//! device layer. They exist because some of them report no (or an unmodelled)
//! `DeviceType` on feature `0x0005`, so the protocol cannot name a kind — and
//! kind decides the capture plane. This table is the direct-path sibling of
//! the receiver pairing register: a per-product identity fact claimed here,
//! which `openlogi-device` converts to a `DeviceKind` at its own boundary.

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
}

impl KeypadDescriptor {
    const fn logitech(product_id: u16, name: &'static str) -> Self {
        Self {
            vendor_id: LOGITECH_VENDOR_ID,
            product_id,
            name,
        }
    }
}

/// All standalone keypad identities OpenLogi recognizes.
pub const KEYPADS: &[KeypadDescriptor] = &[
    // MX Creative Keypad — wired USB-C only, HID++ 4.5. Its `0x0005` marketing
    // name reads correctly, but its DeviceType is `None`, so this entry is the
    // only kind source for it (see `openlogi-device`'s direct-path resolve).
    KeypadDescriptor::logitech(0xc354, "MX Creative Keypad"),
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
    use super::{KEYPADS, find_keypad};
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
    fn lookup_requires_the_matching_vendor_and_product() {
        assert!(find_keypad(0xffff, 0xc354).is_none());
        assert!(find_keypad(LOGITECH_VENDOR_ID, 0xc355).is_none());
    }
}
