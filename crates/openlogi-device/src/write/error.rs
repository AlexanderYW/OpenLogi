use hidpp::channel::ChannelError;
use hidpp::protocol::v20::{ErrorType, Hidpp20Error};

// WriteError, HidppOperation, and HidppFeatureErrorKind are pure IPC wire
// data with no HID++/backend I/O, so they live in
// `openlogi_core::hid::error`; the write path names them through here. The
// conversions below stay here because they name `hidpp` and backend types,
// which `openlogi-core` must never depend on.
pub use openlogi_core::hid::{HidppFeatureErrorKind, HidppOperation, WriteError};

fn hidpp_feature_error_kind(kind: ErrorType) -> HidppFeatureErrorKind {
    match kind {
        ErrorType::NoError => HidppFeatureErrorKind::NoError,
        ErrorType::Unknown => HidppFeatureErrorKind::Unknown,
        ErrorType::InvalidArgument => HidppFeatureErrorKind::InvalidArgument,
        ErrorType::OutOfRange => HidppFeatureErrorKind::OutOfRange,
        ErrorType::HwError => HidppFeatureErrorKind::HwError,
        ErrorType::LogitechInternal => HidppFeatureErrorKind::LogitechInternal,
        ErrorType::InvalidFeatureIndex => HidppFeatureErrorKind::InvalidFeatureIndex,
        ErrorType::InvalidFunctionId => HidppFeatureErrorKind::InvalidFunctionId,
        ErrorType::Busy => HidppFeatureErrorKind::Busy,
        ErrorType::Unsupported => HidppFeatureErrorKind::Unsupported,
        _ => HidppFeatureErrorKind::Unrecognized,
    }
}

pub(crate) fn classify_hidpp_error(
    error: Hidpp20Error,
    operation: HidppOperation,
    feature_hex: u16,
) -> WriteError {
    match error {
        Hidpp20Error::Feature(kind) => WriteError::HidppFeature {
            operation,
            feature_hex,
            kind: hidpp_feature_error_kind(kind),
        },
        Hidpp20Error::UnsupportedResponse => WriteError::UnsupportedResponse {
            operation,
            feature_hex,
        },
        Hidpp20Error::Channel(error) => classify_channel_error(error, operation),
        _ => WriteError::Hidpp(format!("{error:?}")),
    }
}

/// A transport failure of `operation`: a timeout is
/// [`WriteError::RequestTimedOut`], anything else an opaque
/// [`WriteError::Hidpp`].
pub(crate) fn classify_channel_error(error: ChannelError, operation: HidppOperation) -> WriteError {
    match error {
        ChannelError::Timeout => WriteError::RequestTimedOut { operation },
        other => WriteError::Hidpp(format!("{other:?}")),
    }
}

#[cfg(test)]
mod tests;
