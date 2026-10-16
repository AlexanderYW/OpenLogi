use hidpp::channel::ChannelError;
use hidpp::protocol::v20::Hidpp20Error;

use super::{HidppOperation, WriteError, classify_hidpp_error};

/// A feature call that timed out is the same timeout a raw write reports, so
/// a caller sees `RequestTimedOut` however the write reached the device.
#[test]
fn a_feature_call_timeout_is_a_request_timeout() {
    let error = classify_hidpp_error(
        Hidpp20Error::Channel(ChannelError::Timeout),
        HidppOperation::WriteBrightness,
        0x8040,
    );

    assert!(matches!(
        error,
        WriteError::RequestTimedOut {
            operation: HidppOperation::WriteBrightness
        }
    ));
}

#[test]
fn any_other_channel_failure_stays_opaque() {
    let error = classify_hidpp_error(
        Hidpp20Error::Channel(ChannelError::HidppNotSupported),
        HidppOperation::WriteBrightness,
        0x8040,
    );

    assert!(matches!(error, WriteError::Hidpp(_)));
}
