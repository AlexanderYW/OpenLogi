//! `openlogi fixture record case` capture and publication boundary.

use std::fmt::Write as _;
use std::io;
use std::path::PathBuf;

use anyhow::{Context, Result, anyhow, bail};
use clap::{Args, ValueEnum};
use openlogi_core::device::DeviceInventory;
use openlogi_core::single_instance::{self, InstanceGuard, Role};
use openlogi_device::write::{
    self, FeatureEntry, FirmwareEntity, ReprogControlEntry, ScrollWheelMode, WriteError,
};
use openlogi_device::{
    BacklightState, BrightnessState, DeviceRoute, DpiInfo, HidBackend, SmartShiftStatus,
};
use openlogi_fixture::HidCassette;
use openlogi_hid::recording::{
    HidCassetteAudit, HidCassetteIdentityPlan, NativeRecorder, NativeRecording,
};
use openlogi_ipc::client::{self, ConnectError};

use super::target_selection::{self, FixtureTarget};

mod audit;
#[cfg(test)]
mod corpus_tests;
mod replay;

pub(super) const DEFAULT_RECORDING_CAPACITY: usize = 8_192;
const MAX_RECORDING_CAPACITY: usize = 65_536;

/// Arguments for one read-only HID++ cassette capture.
#[derive(Debug, Args)]
pub struct RecordCaseArgs {
    /// Production read operation to record.
    #[arg(long, value_enum)]
    pub operation: FixtureOperation,
    /// Human-readable cassette name.
    #[arg(long)]
    pub name: String,
    /// Logical channel identifier used by replay topology.
    #[arg(long)]
    pub channel: String,
    /// Final JSON cassette path.
    #[arg(long)]
    pub output: PathBuf,
    /// Case-insensitive exact display name, or exact rendered device route.
    #[arg(long)]
    pub device: Option<String>,
    /// Maximum number of in-memory recording events.
    #[arg(
        long,
        default_value_t = DEFAULT_RECORDING_CAPACITY,
        value_parser = parse_capacity
    )]
    pub capacity: usize,
    /// Replace an existing output after every safety gate passes.
    #[arg(long)]
    pub force: bool,
}

/// Read-only production operations accepted by fixture case capture.
#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum FixtureOperation {
    /// Walk the device's HID++ feature table.
    FeatureTable,
    /// Read every firmware entity.
    FirmwareEntities,
    /// Read every reprogrammable control.
    ReprogrammableControls,
    /// Read the raw battery diagnostic result.
    RawBattery,
    /// Read current and supported DPI values.
    DpiInfo,
    /// Read SmartShift status.
    SmartshiftStatus,
    /// Read the high-resolution wheel mode.
    WheelMode,
    /// Read keyboard backlight state.
    BacklightState,
    /// Read the brightness level and range.
    BrightnessState,
}

#[derive(Debug, PartialEq, Eq)]
enum SemanticObservation {
    FeatureTable(Result<Vec<FeatureEntry>, WriteError>),
    FirmwareEntities(Result<Vec<FirmwareEntity>, WriteError>),
    ReprogrammableControls(Result<Vec<ReprogControlEntry>, WriteError>),
    RawBattery(Result<String, WriteError>),
    DpiInfo(Result<DpiInfo, WriteError>),
    SmartshiftStatus(Result<SmartShiftStatus, WriteError>),
    WheelMode(Result<ScrollWheelMode, WriteError>),
    BacklightState(Result<BacklightState, WriteError>),
    BrightnessState(Result<BrightnessState, WriteError>),
}

impl FixtureOperation {
    pub(super) const ALL: [Self; 9] = [
        Self::FeatureTable,
        Self::FirmwareEntities,
        Self::ReprogrammableControls,
        Self::RawBattery,
        Self::DpiInfo,
        Self::SmartshiftStatus,
        Self::WheelMode,
        Self::BacklightState,
        Self::BrightnessState,
    ];

    pub(super) const fn slug(self) -> &'static str {
        match self {
            Self::FeatureTable => "feature-table",
            Self::FirmwareEntities => "firmware-entities",
            Self::ReprogrammableControls => "reprogrammable-controls",
            Self::RawBattery => "raw-battery",
            Self::DpiInfo => "dpi-info",
            Self::SmartshiftStatus => "smartshift-status",
            Self::WheelMode => "wheel-mode",
            Self::BacklightState => "backlight-state",
            Self::BrightnessState => "brightness-state",
        }
    }

    async fn observe(self, backend: &dyn HidBackend, route: &DeviceRoute) -> SemanticObservation {
        match self {
            Self::FeatureTable => {
                SemanticObservation::FeatureTable(write::dump_features(backend, route).await)
            }
            Self::FirmwareEntities => SemanticObservation::FirmwareEntities(
                write::dump_firmware_entities(backend, route).await,
            ),
            Self::ReprogrammableControls => SemanticObservation::ReprogrammableControls(
                write::dump_reprog_controls(backend, route).await,
            ),
            Self::RawBattery => {
                SemanticObservation::RawBattery(write::read_battery_raw(backend, route).await)
            }
            Self::DpiInfo => {
                SemanticObservation::DpiInfo(write::get_dpi_info(backend, route).await)
            }
            Self::SmartshiftStatus => SemanticObservation::SmartshiftStatus(
                write::get_smartshift_status(backend, route).await,
            ),
            Self::WheelMode => {
                SemanticObservation::WheelMode(write::get_scroll_wheel_mode(backend, route).await)
            }
            Self::BacklightState => {
                SemanticObservation::BacklightState(write::get_backlight(backend, route).await)
            }
            Self::BrightnessState => {
                SemanticObservation::BrightnessState(write::get_brightness(backend, route).await)
            }
        }
    }
}

impl SemanticObservation {
    fn ensure_replayable(&self) -> Result<()> {
        let replayable = match self {
            Self::FeatureTable(result) => result.as_ref().err().is_none_or(replayable_error),
            Self::FirmwareEntities(Ok(entities)) => entities.iter().all(|entity| match entity {
                FirmwareEntity::Readable { .. } => true,
                FirmwareEntity::Unreadable { error, .. } => replayable_error(error),
            }),
            Self::FirmwareEntities(Err(error))
            | Self::ReprogrammableControls(Err(error))
            | Self::RawBattery(Err(error))
            | Self::DpiInfo(Err(error))
            | Self::SmartshiftStatus(Err(error))
            | Self::WheelMode(Err(error))
            | Self::BacklightState(Err(error))
            | Self::BrightnessState(Err(error)) => replayable_error(error),
            Self::ReprogrammableControls(Ok(_))
            | Self::RawBattery(Ok(_))
            | Self::DpiInfo(Ok(_))
            | Self::SmartshiftStatus(Ok(_))
            | Self::WheelMode(Ok(_))
            | Self::BacklightState(Ok(_))
            | Self::BrightnessState(Ok(_)) => true,
        };
        if replayable {
            Ok(())
        } else {
            bail!(
                "capture failed with a host, open, disconnect, timeout, interruption, or \
                 non-replayable protocol error; no fixture was written"
            )
        }
    }
}

fn replayable_error(error: &WriteError) -> bool {
    matches!(
        error,
        WriteError::FeatureUnsupported { .. }
            | WriteError::EmptyDpiList
            | WriteError::HidppFeature { .. }
            | WriteError::UnsupportedResponse { .. }
    )
}

#[derive(Clone, Debug)]
pub(super) struct TargetCandidate {
    route: DeviceRoute,
    name: String,
    receiver_vendor_id: u16,
    receiver_product_id: u16,
}

/// A selected target holding agent ownership until all direct capture is done.
pub(super) struct CaptureTarget {
    target: TargetCandidate,
    /// Routes of every other online candidate seen when the target was chosen.
    peers: Vec<DeviceRoute>,
    _agent_guard: InstanceGuard,
}

impl CaptureTarget {
    pub(super) fn route(&self) -> &DeviceRoute {
        &self.target.route
    }

    /// Routes of the other online candidates present at selection.
    pub(super) fn peer_routes(&self) -> &[DeviceRoute] {
        &self.peers
    }
}

impl FixtureTarget for TargetCandidate {
    fn route(&self) -> &DeviceRoute {
        &self.route
    }

    fn display_name(&self) -> &str {
        &self.name
    }
}

#[derive(Clone, Debug)]
struct SanitizedCandidate {
    cassette: HidCassette,
    audit: HidCassetteAudit,
}

pub async fn run(args: RecordCaseArgs) -> Result<()> {
    validate_metadata(&args)?;
    super::output::ensure_output_available(&args.output, args.force)?;
    let target = prepare_contribution_target(args.device.as_deref()).await?;
    let cassette = capture_for_contribution(
        args.operation,
        &target,
        &args.name,
        &args.channel,
        args.capacity,
        &HidCassetteIdentityPlan::default(),
    )
    .await?;
    super::output::write_json_atomically(&args.output, &cassette, args.force, "HID cassette")?;

    println!(
        "Recorded `{}` for {:?} to {}.",
        args.name,
        args.operation,
        args.output.display()
    );
    println!(
        "Self-replay selected exactly one sanitized channel, matched the production operation, \
         and consumed every required exchange."
    );
    println!(
        "Self-replay proves deterministic completeness, not semantic correctness; review the \
         cassette before committing it."
    );
    Ok(())
}

pub(super) async fn prepare_contribution_target(selector: Option<&str>) -> Result<CaptureTarget> {
    let agent_guard = acquire_capture_ownership().await?;
    eprintln!(
        "warning: fixture case capture reads hardware directly with this CLI process's own HID \
         permission and identity, not the OpenLogi agent"
    );
    eprintln!(
        "warning: discovery may enable wireless notifications and request arrival reports on \
         connected receivers before target selection; notification flags are not restored. \
         Captured operations do not change device settings or pairings."
    );
    let inventories = openlogi_hid::enumerate()
        .await
        .map_err(|_| anyhow!("failed to enumerate HID++ devices for direct fixture capture"))?;
    let candidates = online_targets(&inventories);
    let target = target_selection::select_target(&candidates, selector)?;
    let peers = candidates
        .into_iter()
        .map(|candidate| candidate.route)
        .filter(|route| *route != target.route)
        .collect();
    Ok(CaptureTarget {
        target,
        peers,
        _agent_guard: agent_guard,
    })
}

pub(super) async fn capture_for_contribution(
    operation: FixtureOperation,
    target: &CaptureTarget,
    name: &str,
    channel: &str,
    capacity: usize,
    identity_plan: &HidCassetteIdentityPlan,
) -> Result<HidCassette> {
    let (recording, observation) = capture(operation, target.route(), capacity).await?;
    let candidates = audit::sanitize_recording_with_plan(recording, name, channel, identity_plan)?;
    replay::select_self_replaying(operation, &target.target, &observation, candidates).await
}

fn validate_metadata(args: &RecordCaseArgs) -> Result<()> {
    if args.name.trim().is_empty() {
        bail!("--name must not be empty");
    }
    if args.channel.trim().is_empty() {
        bail!("--channel must not be empty");
    }
    Ok(())
}

async fn acquire_capture_ownership() -> Result<InstanceGuard> {
    // The agent acquires this same lock before any HID I/O. An endpoint probe
    // alone misses both early startup and a relaunch after the probe returns.
    let guard = single_instance::acquire(Role::Agent).context(
        "refusing direct fixture capture: could not take the agent's instance lock; \
         stop the OpenLogi agent and any other fixture capture before retrying",
    )?;
    match client::probe_version().await {
        Err(ConnectError::Endpoint(error)) if endpoint_is_unreachable(&error) => Ok(guard),
        Ok(_) | Err(_) => bail!(
            "refusing direct fixture capture because the agent endpoint is active or accepted a \
             connection without completing a healthy handshake; this command uses the CLI's own \
             HID permission and identity, so stop the OpenLogi agent before retrying"
        ),
    }
}

fn endpoint_is_unreachable(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused
    )
}

fn online_targets(inventories: &[DeviceInventory]) -> Vec<TargetCandidate> {
    inventories
        .iter()
        .flat_map(|inventory| {
            inventory
                .paired
                .iter()
                .filter(|device| device.online)
                .filter_map(|device| {
                    let route = DeviceRoute::for_slot(inventory, device.slot)?;
                    if matches!(route, DeviceRoute::RawHid { .. }) {
                        return None;
                    }
                    Some(TargetCandidate {
                        route,
                        name: device
                            .codename
                            .clone()
                            .unwrap_or_else(|| format!("Slot {}", device.slot)),
                        receiver_vendor_id: inventory.receiver.vendor_id,
                        receiver_product_id: inventory.receiver.product_id,
                    })
                })
        })
        .collect()
}

async fn capture(
    operation: FixtureOperation,
    route: &DeviceRoute,
    capacity: usize,
) -> Result<(NativeRecording, SemanticObservation)> {
    let recorder = NativeRecorder::new(capacity).context("could not start bounded HID recorder")?;
    let backend = recorder.backend();
    let observation = operation.observe(&*backend, route).await;
    drop(backend);
    let recording = recorder
        .finish()
        .context("could not finalize bounded HID recording")?;
    observation.ensure_replayable()?;
    Ok((recording, observation))
}

fn parse_capacity(value: &str) -> Result<usize, String> {
    let capacity = value
        .parse::<usize>()
        .map_err(|_| "capacity must be a positive integer".to_string())?;
    if !(1..=MAX_RECORDING_CAPACITY).contains(&capacity) {
        return Err(format!(
            "capacity must be between 1 and {MAX_RECORDING_CAPACITY} events"
        ));
    }
    Ok(capacity)
}

fn uppercase_hex(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(output, "{byte:02X}");
    }
    output
}

#[cfg(test)]
mod tests {
    use openlogi_core::device::{DeviceKind, PairedDevice, ReceiverInfo};
    use openlogi_device::write::{HidppFeatureErrorKind, HidppOperation};

    use super::*;

    fn direct(name: &str, product_id: u16) -> TargetCandidate {
        TargetCandidate {
            route: DeviceRoute::Direct {
                vendor_id: 0x046d,
                product_id,
                identity: None,
            },
            name: name.to_string(),
            receiver_vendor_id: 0x046d,
            receiver_product_id: product_id,
        }
    }

    fn bolt(name: &str, receiver_uid: &str, slot: u8) -> TargetCandidate {
        TargetCandidate {
            route: DeviceRoute::Bolt {
                receiver_uid: receiver_uid.to_string(),
                slot,
            },
            name: name.to_string(),
            receiver_vendor_id: 0x046d,
            receiver_product_id: 0xc548,
        }
    }

    #[test]
    fn target_selection_requires_one_exact_unambiguous_candidate() {
        let only = direct("MX Master 3S", 0xb034);
        assert_eq!(
            target_selection::select_target(std::slice::from_ref(&only), None)
                .expect("one candidate is selected")
                .route,
            only.route
        );

        let choices = vec![only.clone(), direct("MX Keys S", 0xb35b)];
        target_selection::select_target(&choices, None)
            .expect_err("multiple candidates require --device");
        assert_eq!(
            target_selection::select_target(&choices, Some("mx master 3s"))
                .expect("case-insensitive exact name matches")
                .route,
            only.route
        );
        assert!(
            target_selection::select_target(&choices, Some("Master"))
                .expect_err("substrings must not select a target")
                .to_string()
                .contains("no candidate exactly matched")
        );
        assert_eq!(
            target_selection::select_target(&choices, Some("direct 046d:b35b"))
                .expect("exact rendered route matches")
                .name,
            "MX Keys S"
        );
        target_selection::select_target(&choices, Some("DIRECT 046D:B35B"))
            .expect_err("rendered routes require an exact match");
    }

    /// Two identical keypads share a name and a privacy-safe label, so the
    /// candidate list shows each one's full route — the exact `--device`
    /// value that picks it. A lone device keeps the safe label.
    #[test]
    fn identical_devices_list_the_route_that_selects_each_one() {
        let keypad = |unit| TargetCandidate {
            route: DeviceRoute::Direct {
                vendor_id: 0x046d,
                product_id: 0xc354,
                identity: Some(openlogi_core::device_order::DeviceIdentity::Unit([
                    0x4f, 0x4c, 0x44, unit,
                ])),
            },
            ..direct("MX Keypad", 0xc354)
        };
        let choices = vec![keypad(1), keypad(2), direct("MX Keys S", 0xb35b)];

        let error = target_selection::select_target(&choices, Some("MX Keypad"))
            .expect_err("the shared name matches both keypads")
            .to_string();
        assert!(
            error.contains("(direct 046d:c354 (unit:4f4c4401))"),
            "{error}"
        );
        assert!(
            error.contains("(direct 046d:c354 (unit:4f4c4402))"),
            "{error}"
        );
        assert!(error.contains("(direct 046d:b35b)"), "{error}");

        let picked =
            target_selection::select_target(&choices, Some("direct 046d:c354 (unit:4f4c4402)"))
                .expect("the listed route selects one keypad");
        assert_eq!(picked.route, keypad(2).route);
    }

    /// Two devices on the same slot of different receivers share a safe label
    /// but are not twins: the list must never fall back to their receiver UIDs.
    #[test]
    fn same_slot_devices_on_different_receivers_keep_the_safe_label() {
        let choices = vec![
            bolt("MX Master 3S", "OL-BOLT-UID-0001", 1),
            bolt("MX Keys S", "OL-BOLT-UID-0002", 1),
        ];

        let error = target_selection::select_target(&choices, None)
            .expect_err("two candidates need --device")
            .to_string();
        assert!(!error.contains("OL-BOLT-UID"), "{error}");
        assert_eq!(
            error.matches("(Bolt receiver slot 1)").count(),
            2,
            "{error}"
        );
    }

    #[test]
    fn only_definitively_unreachable_agent_endpoints_permit_capture() {
        assert!(endpoint_is_unreachable(&io::Error::from(
            io::ErrorKind::NotFound
        )));
        assert!(endpoint_is_unreachable(&io::Error::from(
            io::ErrorKind::ConnectionRefused
        )));
        assert!(!endpoint_is_unreachable(&io::Error::from(
            io::ErrorKind::PermissionDenied
        )));
        assert!(!endpoint_is_unreachable(&io::Error::other(
            "endpoint resolution failed"
        )));
    }

    // Unix socket paths honor XDG_CONFIG_HOME; Windows uses a fixed pipe name
    // and cannot isolate this endpoint test from a real agent that way.
    #[cfg(unix)]
    #[test]
    fn direct_capture_ownership() {
        use openlogi_core::single_instance::InstanceError;

        const TEST: &str = "cmd::fixture::record_case::tests::direct_capture_ownership";
        const MODE: &str = "OPENLOGI_CAPTURE_OWNERSHIP_TEST";
        let child = |mode: &str| {
            let mut command = std::process::Command::new(std::env::current_exe().unwrap());
            command
                .args(["--exact", TEST, "--nocapture"])
                .env(MODE, mode);
            command
        };
        match std::env::var(MODE).as_deref() {
            Ok("contender") => {
                // This is the same lock acquisition that precedes agent HID I/O.
                assert!(matches!(
                    single_instance::acquire(Role::Agent),
                    Err(InstanceError::AlreadyRunning { .. })
                ));
                return;
            }
            Ok("exercise") => {}
            _ => {
                let home = tempfile::tempdir().unwrap();
                let output = child("exercise")
                    .env("XDG_CONFIG_HOME", home.path())
                    .env_remove("XDG_RUNTIME_DIR")
                    .env(
                        openlogi_core::env::PROFILE,
                        openlogi_core::paths::Profile::Production.env_value(),
                    )
                    .output()
                    .unwrap();
                assert!(
                    output.status.success(),
                    "{}\n{}",
                    String::from_utf8_lossy(&output.stdout),
                    String::from_utf8_lossy(&output.stderr)
                );
                return;
            }
        }

        openlogi_core::worker::runtime().unwrap().block_on(async {
            {
                let _agent = single_instance::acquire(Role::Agent).unwrap();
                let error = prepare_contribution_target(None)
                    .await
                    .err()
                    .expect("an agent starting before its socket binds must exclude capture");
                assert!(matches!(
                    error.downcast_ref::<InstanceError>(),
                    Some(InstanceError::AlreadyRunning { .. })
                ));
            }
            {
                let _capture = CaptureTarget {
                    target: direct("Synthetic", 0xb034),
                    peers: Vec::new(),
                    _agent_guard: acquire_capture_ownership().await.unwrap(),
                };
                let output = child("contender").output().unwrap();
                assert!(
                    output.status.success(),
                    "agent must remain excluded after the endpoint check: {}",
                    String::from_utf8_lossy(&output.stderr)
                );
            }
            // Successful capture scope releases ownership.
            drop(single_instance::acquire(Role::Agent).unwrap());

            // An endpoint accepting connections but not serving a handshake
            // must still fail closed, even when no agent holds the lock.
            let _listener = openlogi_ipc::transport::bind().unwrap();
            acquire_capture_ownership()
                .await
                .err()
                .expect("an unresponsive endpoint must refuse capture");
            drop(single_instance::acquire(Role::Agent).unwrap());

            // Cancelling a pending admission also releases the acquired lock.
            let mut pending = Box::pin(acquire_capture_ownership());
            assert!(futures::poll!(&mut pending).is_pending());
            assert!(matches!(
                single_instance::acquire(Role::Agent),
                Err(InstanceError::AlreadyRunning { .. })
            ));
            drop(pending);
            drop(single_instance::acquire(Role::Agent).unwrap());
        });
    }

    #[test]
    fn semantic_observations_accept_only_typed_replayable_errors() {
        SemanticObservation::FeatureTable(Err(WriteError::FeatureUnsupported {
            feature_hex: 0x0001,
        }))
        .ensure_replayable()
        .expect("typed feature absence can be replayed");
        SemanticObservation::RawBattery(Err(WriteError::HidppFeature {
            operation: HidppOperation::ResolveFeature,
            feature_hex: 0x1004,
            kind: HidppFeatureErrorKind::Unsupported,
        }))
        .ensure_replayable()
        .expect("typed protocol errors can be replayed");
        SemanticObservation::DpiInfo(Err(WriteError::RequestTimedOut {
            operation: HidppOperation::ReadDpi,
        }))
        .ensure_replayable()
        .expect_err("timeouts must fail capture");
        SemanticObservation::RawBattery(Err(WriteError::Hid(
            "unsanitized transport detail".to_string(),
        )))
        .ensure_replayable()
        .expect_err("transport errors must fail without becoming observations");
    }

    #[test]
    fn duplicate_names_and_routes_fail_closed_without_printing_receiver_identity() {
        let duplicate_names = vec![direct("Same", 0xb034), direct("Same", 0xb35b)];
        target_selection::select_target(&duplicate_names, Some("same"))
            .expect_err("duplicate exact names are ambiguous");

        let duplicate_routes = vec![direct("First", 0xb034), direct("Second", 0xb034)];
        let error = target_selection::select_target(&duplicate_routes, Some("First"))
            .expect_err("one display name cannot make a duplicate route safe")
            .to_string();
        assert!(error.contains("selected route is duplicated"));

        let receiver_uid = "RAW-RECEIVER-IDENTITY";
        let receivers = vec![
            bolt("Mouse", receiver_uid, 1),
            bolt("Keyboard", "OTHER-IDENTITY", 2),
        ];
        let error = target_selection::select_target(&receivers, None)
            .expect_err("ambiguous receiver choices fail")
            .to_string();
        assert!(!error.contains(receiver_uid));
        assert!(error.contains("Bolt receiver slot 1"));
    }

    #[test]
    fn online_inventory_candidates_exclude_offline_and_unaddressable_devices() {
        let inventories = vec![DeviceInventory {
            receiver: ReceiverInfo {
                name: "Synthetic direct".to_string(),
                vendor_id: 0x046d,
                product_id: 0xb35b,
                unique_id: None,
            },
            paired: vec![
                PairedDevice {
                    slot: openlogi_device::DIRECT_DEVICE_INDEX,
                    codename: Some("Online".to_string()),
                    wpid: None,
                    kind: DeviceKind::Mouse,
                    online: true,
                    battery: None,
                    model_info: None,
                    capabilities: None,
                    settings_revision: 0,
                },
                PairedDevice {
                    slot: 1,
                    codename: Some("Unaddressable".to_string()),
                    wpid: None,
                    kind: DeviceKind::Mouse,
                    online: true,
                    battery: None,
                    model_info: None,
                    capabilities: None,
                    settings_revision: 0,
                },
                PairedDevice {
                    slot: openlogi_device::DIRECT_DEVICE_INDEX,
                    codename: Some("Offline".to_string()),
                    wpid: None,
                    kind: DeviceKind::Mouse,
                    online: false,
                    battery: None,
                    model_info: None,
                    capabilities: None,
                    settings_revision: 0,
                },
            ],
        }];

        let candidates = online_targets(&inventories);
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].name, "Online");
    }
}
