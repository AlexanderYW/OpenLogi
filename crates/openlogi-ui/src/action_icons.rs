//! The vendored Actions Ring glyphs, as a GPUI [`AssetSource`].
//!
//! Both frontends draw ring slots, so both need these bytes; nothing else in
//! either app's artwork is shared, which is why this source serves *only*
//! `action-icons/` and reports every other path as absent. A frontend needing
//! more composes this with its own source rather than growing this one —
//! anything added here is added to the overlay too.
//!
//! The bytes are `openlogi-action-icons`' embedded table, which the agent also
//! reads to draw keypad LCD keys.

use std::borrow::Cow;

use gpui::{AssetSource, Result, SharedString};
use openlogi_action_icons::{embedded_icon, embedded_icon_paths};

/// The Actions Ring's centre cancel mark. Both processes draw that target —
/// the overlay for real, the settings app as a preview — and they have to be
/// the same mark, so neither side names the asset itself.
pub const RING_CANCEL_ICON: &str = "action-icons/x.svg";

/// GPUI asset source for the embedded ring glyphs, and nothing else.
pub struct ActionIcons;

impl AssetSource for ActionIcons {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        Ok(embedded_icon(path).map(Cow::Borrowed))
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        Ok(embedded_icon_paths()
            .filter(|candidate| candidate.starts_with(path))
            .map(SharedString::from)
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;

    use openlogi_core::binding::ActionRingIcon;

    use super::*;

    #[test]
    fn the_shared_ring_cancel_icon_is_embedded() {
        // Both processes render this path; a rename that missed the array
        // would blank the overlay's cancel target rather than fail to build.
        let loaded = ActionIcons.load(RING_CANCEL_ICON);
        assert!(
            matches!(loaded, Ok(Some(_))),
            "{RING_CANCEL_ICON} is missing"
        );
    }

    #[test]
    fn every_ring_gallery_icon_is_embedded() {
        for icon in ActionRingIcon::ALL {
            let loaded = ActionIcons.load(icon.asset_path());
            assert!(
                matches!(loaded, Ok(Some(_))),
                "missing embedded asset for {icon:?}"
            );
        }
    }
}
