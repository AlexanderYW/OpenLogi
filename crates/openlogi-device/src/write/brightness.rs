//! HID++ `0x8040 BrightnessControl` reads and writes.
//!
//! `get_info` reports the range and `get_brightness` the level, so a read
//! answers with both: the level is only meaningful against the range the
//! device itself advertised. A write echoes by reading back — firmware is
//! allowed to clamp, and the read-back level is the applied value.

use std::sync::Arc;

use hidpp::channel::HidppChannel;
use hidpp::device::Device;
use hidpp::feature::CreatableFeature;
use hidpp::feature::brightness_control::BrightnessControlFeature;
use tracing::debug;

use super::{HidppOperation, WriteError, classify_hidpp_error, open_feature};
use crate::SharedChannel;
use openlogi_core::hid::BrightnessState;

/// Read the brightness state of the device already reached by `shared`.
pub async fn get_brightness_on(shared: &SharedChannel) -> Result<BrightnessState, WriteError> {
    let feature = open_brightness_feature(shared.channel(), shared.device_index()).await?;
    read_state(&feature).await
}

/// Set `shared`'s brightness and return the state the device reports
/// afterwards — the applied level, since firmware may clamp the request.
pub async fn set_brightness_on(
    shared: &SharedChannel,
    level: u16,
) -> Result<BrightnessState, WriteError> {
    let index = shared.device_index();
    let feature = open_brightness_feature(shared.channel(), index).await?;
    feature.set_brightness(level).await.map_err(|error| {
        classify_hidpp_error(
            error,
            HidppOperation::WriteBrightness,
            BrightnessControlFeature::ID,
        )
    })?;
    let applied = read_state(&feature).await?;
    debug!(
        index,
        requested = level,
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

async fn read_state(feature: &BrightnessControlFeature) -> Result<BrightnessState, WriteError> {
    let info = feature.get_info().await.map_err(|error| {
        classify_hidpp_error(
            error,
            HidppOperation::ReadBrightness,
            BrightnessControlFeature::ID,
        )
    })?;
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
