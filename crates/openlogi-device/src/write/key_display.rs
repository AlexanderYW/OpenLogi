//! Images on a keypad's LCD keys — the MX Creative Keypad's vendor image
//! write.
//!
//! The keys are tiles of one panel, and no HID++ feature drives it: an image
//! goes out as one or more 4095-byte output reports `0x14` on the vendor
//! channel (`ff 02`) that also carries the grid-key presses. The first packet
//! is
//!
//! ```text
//! 14 ff 02 2b [part] 01 00 01 00 [x] [y] [w] [h] 00 [len] [jpeg…]
//! ```
//!
//! with big-endian `u16` fields and the JPEG from byte 20; continuation
//! packets carry the rest from byte 5. `part` is the packet's 1-based index
//! `| 0x20`, `| 0x80` on the first packet and `| 0x40` on the last. The device
//! acknowledges the image with a `13 ff 02 2b …` report.
//!
//! The layout follows `node-logitech-mx-creative-console` (MIT), and was
//! verified on hardware with `openlogi-hid`'s `mx_keypad_lcd_probe` (2026-10):
//! single-packet JPEG tiles at the registry's key positions filled each key
//! edge to edge, each answered by `13 ff 02 2b c1 00 01`. Where the panel's
//! keys sit is the product's identity fact, owned by
//! [`openlogi_device_registry::keypad`].

use std::time::Duration;

use openlogi_core::binding::GridKey;
pub use openlogi_device_registry::keypad::KeyDisplay;
use openlogi_device_registry::keypad::find_keypad;
use tokio::sync::oneshot;
use tracing::debug;

use super::{HidppOperation, WriteError, classify_channel_error};
use crate::keypad_vendor::{VendorFunction, input_payload, output_header};
use crate::{DeviceRoute, SharedChannel};

/// Bytes 0–3 of every image packet.
const IMAGE_HEADER: [u8; 4] = output_header(VendorFunction::Image);
/// Every image packet is a full-width report.
const PACKET_LEN: usize = 4095;
const FIRST_HEADER_LEN: usize = 20;
const NEXT_HEADER_LEN: usize = 5;
/// How long the device may take to acknowledge an image after its last
/// packet. Acknowledgements arrive within milliseconds on hardware.
const ACK_TIMEOUT: Duration = Duration::from_secs(2);

/// The LCD key panel of the device at `route`, or `None` when its keys are not
/// displays. Only a direct (wired) keypad the registry knows has one.
#[must_use]
pub fn key_display_for(route: &DeviceRoute) -> Option<KeyDisplay> {
    let DeviceRoute::Direct {
        vendor_id,
        product_id,
    } = route
    else {
        return None;
    };
    find_keypad(*vendor_id, *product_id).and_then(|keypad| keypad.key_display)
}

/// A baseline JPEG small enough for the image write's `u16` length field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyImageJpeg(Vec<u8>);

impl KeyImageJpeg {
    /// Wrap encoded JPEG bytes, or `None` when they are empty or longer than
    /// the write can declare (`u16::MAX`).
    #[must_use]
    pub fn new(bytes: Vec<u8>) -> Option<Self> {
        (!bytes.is_empty() && u16::try_from(bytes.len()).is_ok()).then_some(Self(bytes))
    }

    /// The encoded bytes.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}

/// Draw `image` on `key` of the panel `display` describes, through the
/// already-open channel `shared`, and wait for the device to acknowledge it.
///
/// `image` must already be encoded at `display.key_size` square; the device
/// places it, it does not scale it. A key off `display`'s grid is
/// [`WriteError::Hidpp`], since the caller derived both from one device.
pub async fn write_key_image_on(
    shared: &SharedChannel,
    display: KeyDisplay,
    key: GridKey,
    image: &KeyImageJpeg,
) -> Result<(), WriteError> {
    let (row, col) = key.position();
    let (x, y) = u16::try_from(row)
        .ok()
        .zip(u16::try_from(col).ok())
        .and_then(|(row, col)| display.key_origin(row, col))
        .ok_or_else(|| WriteError::Hidpp(format!("{key:?} is off the panel's key grid")))?;
    let packets = image_packets(x, y, display.key_size, image);

    let channel = shared.channel();
    let (ack_tx, ack_rx) = oneshot::channel();
    let ack_tx = std::sync::Mutex::new(Some(ack_tx));
    let _ack_listener = channel.add_raw_report_listener_guarded(move |report| {
        if is_image_ack(report)
            && let Some(tx) = ack_tx.lock().ok().and_then(|mut slot| slot.take())
        {
            let _ = tx.send(());
        }
    });

    for packet in &packets {
        let written = channel
            .write_raw_report(packet)
            .await
            .map_err(|error| classify_channel_error(error, HidppOperation::WriteKeyImage))?;
        if written != packet.len() {
            return Err(WriteError::Hidpp(format!(
                "key image report wrote {written} of {} bytes",
                packet.len()
            )));
        }
    }
    tokio::time::timeout(ACK_TIMEOUT, ack_rx)
        .await
        .ok()
        .and_then(Result::ok)
        .ok_or(WriteError::RequestTimedOut {
            operation: HidppOperation::WriteKeyImage,
        })?;
    debug!(
        ?key,
        bytes = image.as_bytes().len(),
        packets = packets.len(),
        "key image written"
    );
    Ok(())
}

/// Whether `report` acknowledges an image write.
fn is_image_ack(report: &[u8]) -> bool {
    input_payload(report, VendorFunction::Image).is_some()
}

fn part_byte(index: u8, first: bool, last: bool) -> u8 {
    let mut byte = index | 0x20;
    if first {
        byte |= 0x80;
    }
    if last {
        byte |= 0x40;
    }
    byte
}

fn packet_header(part: u8) -> [u8; 5] {
    let [id, channel_hi, channel_lo, function] = IMAGE_HEADER;
    [id, channel_hi, channel_lo, function, part]
}

/// Split `image` into the vendor write's packets for a `size`-square tile at
/// `(x, y)`.
fn image_packets(x: u16, y: u16, size: u16, image: &KeyImageJpeg) -> Vec<Vec<u8>> {
    let jpeg = image.as_bytes();
    // `KeyImageJpeg` bounds the length by construction.
    let total = u16::try_from(jpeg.len()).unwrap_or(u16::MAX);
    let first_len = jpeg.len().min(PACKET_LEN - FIRST_HEADER_LEN);
    let continuations = (jpeg.len() - first_len).div_ceil(PACKET_LEN - NEXT_HEADER_LEN);

    let mut first = vec![0u8; PACKET_LEN];
    first[..5].copy_from_slice(&packet_header(part_byte(1, true, continuations == 0)));
    first[5..7].copy_from_slice(&0x0100u16.to_be_bytes());
    first[7..9].copy_from_slice(&0x0100u16.to_be_bytes());
    first[9..11].copy_from_slice(&x.to_be_bytes());
    first[11..13].copy_from_slice(&y.to_be_bytes());
    first[13..15].copy_from_slice(&size.to_be_bytes());
    first[15..17].copy_from_slice(&size.to_be_bytes());
    first[18..20].copy_from_slice(&total.to_be_bytes());
    first[FIRST_HEADER_LEN..FIRST_HEADER_LEN + first_len].copy_from_slice(&jpeg[..first_len]);

    let mut packets = vec![first];
    let rest = jpeg[first_len..].chunks(PACKET_LEN - NEXT_HEADER_LEN);
    for (index, chunk) in (2u8..).zip(rest) {
        let last = usize::from(index) - 1 == continuations;
        let mut packet = vec![0u8; PACKET_LEN];
        packet[..NEXT_HEADER_LEN].copy_from_slice(&packet_header(part_byte(index, false, last)));
        packet[NEXT_HEADER_LEN..NEXT_HEADER_LEN + chunk.len()].copy_from_slice(chunk);
        packets.push(packet);
    }
    packets
}

#[cfg(test)]
mod tests;
