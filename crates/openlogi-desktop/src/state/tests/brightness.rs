//! Brightness: a committed level is persisted for the agent to restore and
//! written through the agent, and the device's echo answers it.

use super::*;
use crate::services::ipc::{Command, SetBrightness};
use openlogi_core::hid::BrightnessState;

/// A wired MX Creative Keypad (direct route, numpad kind), with or without
/// measured brightness (`0x8040`). Nothing else about the device matters
/// here — brightness is a device property, not a keypad one.
fn keypad_inventory(brightness: bool) -> DeviceInventory {
    DeviceInventory {
        receiver: ReceiverInfo {
            name: "MX Creative Keypad".to_string(),
            vendor_id: 0x046d,
            product_id: 0xc354,
            unique_id: None,
        },
        paired: vec![PairedDevice {
            slot: openlogi_core::hid::DIRECT_DEVICE_INDEX,
            codename: Some("MX Creative Keypad".to_string()),
            wpid: None,
            kind: DeviceKind::Numpad,
            online: true,
            battery: None,
            model_info: Some(DeviceModelInfo {
                entity_count: 4,
                serial_number: None,
                unit_id: [0x4f, 0x4c, 0x44, 0x02],
                transports: DeviceTransports::default(),
                model_ids: [0xc354, 0, 0],
                extended_model_id: 0,
            }),
            capabilities: Some(Capabilities {
                brightness,
                ..Capabilities::presumed_from_kind(DeviceKind::Numpad)
            }),
            settings_revision: 0,
        }],
    }
}

fn keypad_state(brightness: bool) -> (AppState, tokio::sync::mpsc::UnboundedReceiver<Command>) {
    let resolver = AssetResolver::new();
    let (commands, receiver) = tokio::sync::mpsc::unbounded_channel();
    let state = AppState::new(Sources {
        inventories: &[keypad_inventory(brightness)],
        ..Sources::in_memory(Config::ephemeral(), &resolver, commands)
    });
    (state, receiver)
}

fn drain(receiver: &mut tokio::sync::mpsc::UnboundedReceiver<Command>) -> Vec<Command> {
    std::iter::from_fn(|| receiver.try_recv().ok()).collect()
}

#[test]
fn brightness_write_persists_the_level_and_writes_the_device() {
    let (mut state, mut receiver) = keypad_state(true);
    drain(&mut receiver);
    assert!(state.current_brightness_supported());
    let record = state.current_record().expect("the keypad is selected");
    let key = record.device_key();
    let route = record.route.clone().expect("a direct route");
    let config_key = record
        .persistent_config_key()
        .expect("a unit id makes the keypad persistent")
        .to_string();

    let events = state.commit_brightness(75);

    assert_eq!(events, [StateEvent::BrightnessChanged(key.clone())]);
    // Persisted so the agent can restore it after the keypad power-cycles.
    assert_eq!(state.config.brightness(&config_key), Some(75));
    let commands = drain(&mut receiver);
    let [
        Command::ReloadConfig(_),
        Command::SetBrightness(SetBrightness {
            route: written_route,
            level,
            key: written_key,
        }),
    ] = commands.as_slice()
    else {
        panic!("expected a config reload followed by one brightness write");
    };
    assert_eq!(*written_route, route);
    assert_eq!(*level, 75);
    assert_eq!(*written_key, key);
}

#[test]
fn brightness_write_is_inert_without_the_control() {
    let (mut state, mut receiver) = keypad_state(false);
    drain(&mut receiver);
    assert!(!state.current_brightness_supported());
    let config_key = state
        .current_record()
        .and_then(DeviceRecord::persistent_config_key)
        .expect("persistent")
        .to_string();

    let _ = state.commit_brightness(75);

    assert_eq!(state.config.brightness(&config_key), None);
    assert!(drain(&mut receiver).is_empty());
}

#[test]
fn the_devices_answer_events_the_device_either_way() {
    let (mut state, _receiver) = keypad_state(true);
    let key = state.current_record().expect("keypad").device_key();
    let echoed = BrightnessState {
        level: 60,
        min: 0,
        max: 100,
        steps: 0,
    };

    assert_eq!(
        state.apply_brightness_written(&key, Ok(echoed)),
        [StateEvent::BrightnessChanged(key.clone())]
    );
    assert_eq!(
        state.apply_brightness_written(&key, Err(WriteError::AgentUnavailable)),
        [StateEvent::BrightnessChanged(key)]
    );
}
