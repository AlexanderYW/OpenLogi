//! Reaching one of two identical direct devices by the identity its route
//! carries.

use std::sync::{Arc, Mutex, PoisonError};

use hidpp::channel::HidppChannel;
use openlogi_core::device_order::DeviceIdentity;

use super::*;
use crate::backend::{HotplugStream, NodeId, RawWriter};
use crate::channel::scripted::{ScriptedRawHidChannel, scripted_channel};

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

/// Two identical wired keypads, each opening into its own scripted channel,
/// recording which node was opened.
struct TwinKeypads {
    nodes: Vec<(NodeInfo, [u8; 4])>,
    opened: Mutex<Vec<NodeId>>,
}

impl TwinKeypads {
    fn new(units: &[[u8; 4]]) -> Self {
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
        let (raw, _) = ScriptedRawHidChannel::with_dynamic_responder(keypad_responder(*unit));
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
