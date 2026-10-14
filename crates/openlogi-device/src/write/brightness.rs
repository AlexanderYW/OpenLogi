//! HID++ `0x8040 BrightnessControl` reads and writes.
//!
//! `get_info` reports the range and `get_brightness` the level, so a read
//! answers with both: the level is only meaningful against the range the
//! device itself advertised. Every write is first clamped into
//! [`WritableBrightness`] — never `0`, which resets the MX Creative Keypad —
//! and echoes by reading back: firmware is allowed to clamp too, and the
//! read-back level is the applied value.

use std::sync::Arc;

use hidpp::channel::HidppChannel;
use hidpp::device::Device;
use hidpp::feature::CreatableFeature;
use hidpp::feature::brightness_control::{
    BrightnessCapabilities, BrightnessControlFeature, BrightnessInfo,
};
use tracing::debug;

use super::{HidppOperation, WriteError, classify_hidpp_error, open_feature};
use crate::SharedChannel;
use openlogi_core::hid::{BrightnessState, WritableBrightness};

/// Read the brightness state of the device already reached by `shared`.
pub async fn get_brightness_on(shared: &SharedChannel) -> Result<BrightnessState, WriteError> {
    let feature = open_brightness_feature(shared.channel(), shared.device_index()).await?;
    read_state(&feature).await
}

/// Set `shared`'s brightness and return the state the device reports
/// afterwards — the applied level, since firmware may clamp the request.
/// `level` is clamped into the device's [`WritableBrightness`] first.
pub async fn set_brightness_on(
    shared: &SharedChannel,
    level: u16,
) -> Result<BrightnessState, WriteError> {
    let feature = open_brightness_feature(shared.channel(), shared.device_index()).await?;
    let info = read_info(&feature).await?;
    write_level(&feature, shared.device_index(), info, level).await
}

/// Re-apply a persisted `level` to a device that forgets its brightness on a
/// power cycle, and return the echoed state. A device whose BrightnessControl
/// does not report itself transient keeps its own level — which the user may
/// have changed on the device since — so it is left alone and `None` returns.
pub async fn restore_brightness_on(
    shared: &SharedChannel,
    level: u16,
) -> Result<Option<BrightnessState>, WriteError> {
    let feature = open_brightness_feature(shared.channel(), shared.device_index()).await?;
    let info = read_info(&feature).await?;
    if !info
        .capabilities
        .contains(BrightnessCapabilities::TRANSIENT)
    {
        return Ok(None);
    }
    write_level(&feature, shared.device_index(), info, level)
        .await
        .map(Some)
}

async fn write_level(
    feature: &BrightnessControlFeature,
    index: u8,
    info: BrightnessInfo,
    requested: u16,
) -> Result<BrightnessState, WriteError> {
    let level = WritableBrightness::new(info.min_brightness, info.max_brightness).clamp(requested);
    feature.set_brightness(level).await.map_err(|error| {
        classify_hidpp_error(
            error,
            HidppOperation::WriteBrightness,
            BrightnessControlFeature::ID,
        )
    })?;
    let applied = read_state(feature).await?;
    debug!(
        index,
        requested,
        written = level,
        applied = applied.level,
        "brightness written"
    );
    Ok(applied)
}

async fn open_brightness_feature(
    channel: &Arc<HidppChannel>,
    index: u8,
) -> Result<Arc<BrightnessControlFeature>, WriteError> {
    let mut device = Device::new(Arc::clone(channel), index)
        .await
        .map_err(|_| WriteError::DeviceUnreachable { index })?;
    open_feature::<BrightnessControlFeature>(&mut device).await
}

async fn read_info(feature: &BrightnessControlFeature) -> Result<BrightnessInfo, WriteError> {
    feature.get_info().await.map_err(|error| {
        classify_hidpp_error(
            error,
            HidppOperation::ReadBrightness,
            BrightnessControlFeature::ID,
        )
    })
}

async fn read_state(feature: &BrightnessControlFeature) -> Result<BrightnessState, WriteError> {
    let info = read_info(feature).await?;
    let level = feature.get_brightness().await.map_err(|error| {
        classify_hidpp_error(
            error,
            HidppOperation::ReadBrightness,
            BrightnessControlFeature::ID,
        )
    })?;
    Ok(BrightnessState {
        level,
        min: info.min_brightness,
        max: info.max_brightness,
        steps: info.steps,
    })
}
