//! How a `--device` query names a device, shared by every command that has one.
//!
//! The agent-backed commands (`backlight`) cannot import the hardware-diag
//! module (see `backlight-agent-owner.yml`), and diag must not reach into a
//! command module, so the decision lives in this neutral parent module and
//! both commands consume it. One flag, one meaning.

use openlogi_core::hid::DeviceRoute;

/// The devices a `--device` query names: those whose rendered route it spells
/// out exactly — the only way to tell two identical devices apart, since
/// their names match — or, when none does, those whose name contains the
/// query. Both comparisons ignore case.
pub(crate) fn devices_named<'a, T>(
    devices: &'a [T],
    query: &str,
    route_of: impl Fn(&T) -> Option<&DeviceRoute>,
    name_of: impl Fn(&T) -> &str,
) -> Vec<&'a T> {
    let by_route: Vec<_> = devices
        .iter()
        .filter(|device| {
            route_of(device).is_some_and(|route| route.to_string().eq_ignore_ascii_case(query))
        })
        .collect();
    if !by_route.is_empty() {
        return by_route;
    }
    let needle = query.to_lowercase();
    devices
        .iter()
        .filter(|device| name_of(device).to_lowercase().contains(&needle))
        .collect()
}
