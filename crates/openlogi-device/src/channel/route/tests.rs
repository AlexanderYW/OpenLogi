//! Reaching one of two identical direct devices by the identity its route
//! carries.

use std::sync::{Arc, Mutex, PoisonError};

use hidpp::channel::HidppChannel;
use openlogi_core::device_order::DeviceIdentity;

use super::*;
use crate::backend::{HotplugStream, NodeId, RawWriter};
use crate::channel::scripted::{ScriptedRawHidChannel, feature_error, scripted_channel};

/// Where the scripted keypads list `0x0003` DeviceInformation.
const DEVICE_INFORMATION_INDEX: u8 = 0x02;
const KEYPAD_PID: u16 = 0xc354;

/// A wired keypad answering at `0xff` with `unit_id` and no serial.
fn keypad_responder(unit_id: [u8; 4]) -> impl Fn(&[u8]) -> Option<Vec<u8>> + Send + Sync {
    move |request| {
        if request.len() < 7 || !matches!(request[0], 0x10 | 0x11) || request[1] != 0xff {
            return None;
        }
        let function = request[3] >> 4;
        let feature = u16::from_be_bytes([request[4], request[5]]);
        let mut payload = [0u8; 16];
        match (request[2], function) {
            // Root ping, then the feature lookup.
            (0x00, 0x01) => payload[0] = 4,
            (0x00, 0x00) if feature == 0x0003 => {
                payload[0] = DEVICE_INFORMATION_INDEX;
                payload[2] = 4;
            }
            (0x00, 0x00) => {}
            // getDeviceInfo: entity count, then the unit id; capabilities
            // (byte 14) report no serial.
            (DEVICE_INFORMATION_INDEX, 0x00) => {
                payload[0] = 1;
                payload[1..5].copy_from_slice(&unit_id);
            }
            _ => return None,
        }
        let mut response = vec![0u8; 20];
        response[0] = 0x11;
        response[1..4].copy_from_slice(&request[1..4]);
        response[4..].copy_from_slice(&payload);
        Some(response)
    }
}

/// A wired keypad answering at `0xff` with `unit_id` and no readable serial:
/// `0x0003` advertises the serial capability (byte 14), and the serial
/// function itself answers with a HID++ 2.0 error — what the probe reads as a
/// serial read failure, which it survives by keeping the unit id.
fn keypad_responder_with_unreadable_serial(
    unit_id: [u8; 4],
) -> impl Fn(&[u8]) -> Option<Vec<u8>> + Send + Sync {
    move |request| {
        if request.len() < 7 || !matches!(request[0], 0x10 | 0x11) || request[1] != 0xff {
            return None;
        }
        let function = request[3] >> 4;
        let feature = u16::from_be_bytes([request[4], request[5]]);
        if request[2] == DEVICE_INFORMATION_INDEX && function == 2 {
            return Some(feature_error(request, 7)); // invalidFunction
        }
        let mut payload = [0u8; 16];
        match (request[2], function) {
            // Root ping, then the feature lookup.
            (0x00, 0x01) => payload[0] = 4,
            (0x00, 0x00) if feature == 0x0003 => {
                payload[0] = DEVICE_INFORMATION_INDEX;
                payload[2] = 4;
            }
            (0x00, 0x00) => {}
            // getDeviceInfo: entity count, then the unit id; capabilities
            // (byte 14) advertise a serial the device then refuses to give.
            (DEVICE_INFORMATION_INDEX, 0x00) => {
                payload[0] = 1;
                payload[1..5].copy_from_slice(&unit_id);
                payload[14] = 1;
            }
            _ => return None,
        }
        let mut response = vec![0u8; 20];
        response[0] = 0x11;
        response[1..4].copy_from_slice(&request[1..4]);
        response[4..].copy_from_slice(&payload);
        Some(response)
    }
}

/// Two identical wired keypads, each opening into its own scripted channel,
/// recording which node was opened.
struct TwinKeypads {
    nodes: Vec<(NodeInfo, [u8; 4])>,
    serial_unreadable: bool,
    opened: Mutex<Vec<NodeId>>,
}

impl TwinKeypads {
    fn new(units: &[[u8; 4]]) -> Self {
        Self::build(units, false)
    }

    /// [`Self::new`], with each keypad advertising a serial it refuses to
    /// hand over.
    fn with_unreadable_serial(units: &[[u8; 4]]) -> Self {
        Self::build(units, true)
    }

    fn build(units: &[[u8; 4]], serial_unreadable: bool) -> Self {
        let nodes = units
            .iter()
            .enumerate()
            .map(|(index, unit)| {
                (
                    NodeInfo {
                        id: NodeId::from(format!("keypad-{index}")),
                        vendor_id: LOGITECH_VENDOR_ID,
                        product_id: KEYPAD_PID,
                        usage_page: 0xff43,
                        usage_id: 0x1a02,
                        name: "MX Creative Keypad".into(),
                        manufacturer: Some("Logitech".into()),
                        serial_number: None,
                    },
                    *unit,
                )
            })
            .collect();
        Self {
            nodes,
            serial_unreadable,
            opened: Mutex::new(Vec::new()),
        }
    }

    fn opened(&self) -> Vec<NodeId> {
        self.opened
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

#[hidpp::async_trait]
impl HidBackend for TwinKeypads {
    async fn enumerate(&self) -> Result<Vec<NodeInfo>, BackendError> {
        Ok(self.nodes.iter().map(|(info, _)| info.clone()).collect())
    }

    async fn enumerate_hidpp(&self) -> Result<Vec<NodeInfo>, BackendError> {
        self.enumerate().await
    }

    async fn open_hidpp(&self, node: &NodeInfo) -> Result<Option<Arc<HidppChannel>>, BackendError> {
        let Some((_, unit)) = self.nodes.iter().find(|(info, _)| info.id == node.id) else {
            return Err(BackendError::Disconnected);
        };
        self.opened
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(node.id.clone());
        let responder: Box<dyn Fn(&[u8]) -> Option<Vec<u8>> + Send + Sync> =
            if self.serial_unreadable {
                Box::new(keypad_responder_with_unreadable_serial(*unit))
            } else {
                Box::new(keypad_responder(*unit))
            };
        let (raw, _) = ScriptedRawHidChannel::with_dynamic_responder(responder);
        Ok(Some(scripted_channel(raw).await))
    }

    async fn open_raw_writer(&self, _node: &NodeInfo) -> Result<Box<dyn RawWriter>, BackendError> {
        Err(BackendError::Backend("no raw writer".into()))
    }

    fn watch(&self) -> Result<HotplugStream, BackendError> {
        Ok(Box::new(futures_lite::stream::empty()))
    }
}

fn keypad_route(identity: Option<DeviceIdentity>) -> DeviceRoute {
    DeviceRoute::Direct {
        vendor_id: LOGITECH_VENDOR_ID,
        product_id: KEYPAD_PID,
        identity,
    }
}

const FIRST: [u8; 4] = [0x4f, 0x4c, 0x44, 0x01];
const SECOND: [u8; 4] = [0x4f, 0x4c, 0x44, 0x02];

#[tokio::test]
async fn a_route_reaches_the_twin_whose_identity_it_carries() {
    let backend = TwinKeypads::new(&[FIRST, SECOND]);

    let channel = open_route_channel(&backend, &keypad_route(Some(DeviceIdentity::Unit(SECOND))))
        .await
        .unwrap();

    assert!(channel.is_some());
    assert_eq!(
        backend.opened(),
        [
            NodeId::from("keypad-0".to_owned()),
            NodeId::from("keypad-1".to_owned())
        ],
        "the first twin is read and passed over"
    );
    let channel = channel.unwrap();
    assert_eq!(
        read_device_identity(&channel, DIRECT_DEVICE_INDEX).await,
        Some(DeviceIdentity::Unit(SECOND))
    );
}

#[tokio::test]
async fn an_identity_no_twin_reports_reaches_nothing() {
    let backend = TwinKeypads::new(&[FIRST, SECOND]);

    let channel = open_route_channel(
        &backend,
        &keypad_route(Some(DeviceIdentity::Unit([0x4f, 0x4c, 0x44, 0x09]))),
    )
    .await
    .unwrap();

    assert!(
        channel.is_none(),
        "a missing device is not answered by its twin"
    );
}

#[tokio::test]
async fn a_lone_node_is_its_route_without_reading_its_identity() {
    // Nothing to tell apart: the route's VID/PID reaches the one keypad
    // present with no extra request — what a recorded single-device fixture
    // replays — whichever identity the route names.
    let backend = TwinKeypads::new(&[FIRST]);

    let channel = open_route_channel(&backend, &keypad_route(Some(DeviceIdentity::Unit(SECOND))))
        .await
        .unwrap();

    assert!(channel.is_some());
}

#[tokio::test]
async fn a_route_without_an_identity_takes_the_first_twin() {
    let backend = TwinKeypads::new(&[FIRST, SECOND]);

    let channel = open_route_channel(&backend, &keypad_route(None))
        .await
        .unwrap()
        .unwrap();

    assert_eq!(backend.opened(), [NodeId::from("keypad-0".to_owned())]);
    assert_eq!(
        read_device_identity(&channel, DIRECT_DEVICE_INDEX).await,
        Some(DeviceIdentity::Unit(FIRST))
    );
}

/// The probe keeps the unit id when this very serial read fails — the route a
/// twin carries was built by that probe — so at open time a failed serial read
/// must fall back to the unit id too, not pass the right twin over as
/// "another device of the model".
#[tokio::test]
async fn a_twin_with_an_unreadable_serial_still_matches_by_unit_id() {
    let backend = TwinKeypads::with_unreadable_serial(&[FIRST, SECOND]);

    let channel = open_route_channel(&backend, &keypad_route(Some(DeviceIdentity::Unit(SECOND))))
        .await
        .unwrap();

    assert!(
        channel.is_some(),
        "a failed serial read must not make the named twin unreachable"
    );
    assert_eq!(
        backend.opened(),
        [
            NodeId::from("keypad-0".to_owned()),
            NodeId::from("keypad-1".to_owned())
        ],
        "the first twin is passed over by its unit id, not by its failed serial read"
    );
}
