//! The vendored action glyphs, as embedded bytes.
//!
//! Every process that draws an action draws the same glyph: the two GPUI
//! frontends serve these bytes through `openlogi-ui`'s asset source, and the
//! agent rasterizes them onto a keypad's LCD keys. The table is its own crate
//! so the agent reads it without linking a UI toolkit; `openlogi-core`'s
//! `ActionRingIcon::asset_path` names the paths.
//!
//! Embedding via `include_bytes!` means the paths resolve identically inside a
//! packaged `.app` and from a dev build; a filesystem path would not.

/// Vendored [lucide](https://lucide.dev) icons (ISC license), embedded so they
/// resolve identically in a packaged `.app` and a dev build. Keyed by the
/// `action-icons/` path the GPUI frontends' asset source serves them under. Mostly command glyphs for the binding menus
/// (paste / cut / volume / lock / …), plus the chrome marks gpui-component's
/// bundled `IconName` set does not cover — About-page icons, and `mouse-left` /
/// `mouse-right`, which are lucide's `mouse` with one button filled in.
///
/// `x.svg` duplicates `IconName::Close` on purpose: the overlay does not depend
/// on gpui-component, and its ring cancel target has to match the settings-side
/// preview of the same ring exactly.
#[rustfmt::skip]
const ACTION_ICONS: &[(&str, &[u8])] = &[
    ("action-icons/arrow-left.svg", include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/action-icons/arrow-left.svg"))),
    ("action-icons/arrow-right.svg", include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/action-icons/arrow-right.svg"))),
    ("action-icons/ban.svg", include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/action-icons/ban.svg"))),
    ("action-icons/battery-low.svg", include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/action-icons/battery-low.svg"))),
    ("action-icons/bell.svg", include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/action-icons/bell.svg"))),
    ("action-icons/bluetooth.svg", include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/action-icons/bluetooth.svg"))),
    ("action-icons/book-open.svg", include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/action-icons/book-open.svg"))),
    ("action-icons/bolt.svg", include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/action-icons/bolt.svg"))),
    ("action-icons/bug.svg", include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/action-icons/bug.svg"))),
    ("action-icons/calendar.svg", include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/action-icons/calendar.svg"))),
    ("action-icons/camera.svg", include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/action-icons/camera.svg"))),
    ("action-icons/chevron-left.svg", include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/action-icons/chevron-left.svg"))),
    ("action-icons/chevron-right.svg", include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/action-icons/chevron-right.svg"))),
    ("action-icons/chevrons-down.svg", include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/action-icons/chevrons-down.svg"))),
    ("action-icons/chevrons-left.svg", include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/action-icons/chevrons-left.svg"))),
    ("action-icons/chevrons-right.svg", include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/action-icons/chevrons-right.svg"))),
    ("action-icons/chevrons-up.svg", include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/action-icons/chevrons-up.svg"))),
    ("action-icons/circle-arrow-left.svg", include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/action-icons/circle-arrow-left.svg"))),
    ("action-icons/circle-arrow-right.svg", include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/action-icons/circle-arrow-right.svg"))),
    ("action-icons/circle-dot.svg", include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/action-icons/circle-dot.svg"))),
    ("action-icons/clipboard-paste.svg", include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/action-icons/clipboard-paste.svg"))),
    ("action-icons/copy.svg", include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/action-icons/copy.svg"))),
    ("action-icons/dot.svg", include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/action-icons/dot.svg"))),
    ("action-icons/file.svg", include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/action-icons/file.svg"))),
    ("action-icons/folder.svg", include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/action-icons/folder.svg"))),
    ("action-icons/gauge.svg", include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/action-icons/gauge.svg"))),
    ("action-icons/globe.svg", include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/action-icons/globe.svg"))),
    ("action-icons/grid-3x3.svg", include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/action-icons/grid-3x3.svg"))),
    ("action-icons/heart.svg", include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/action-icons/heart.svg"))),
    ("action-icons/keyboard.svg", include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/action-icons/keyboard.svg"))),
    ("action-icons/layers.svg", include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/action-icons/layers.svg"))),
    ("action-icons/layout-grid.svg", include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/action-icons/layout-grid.svg"))),
    ("action-icons/list-checks.svg", include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/action-icons/list-checks.svg"))),
    ("action-icons/lock.svg", include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/action-icons/lock.svg"))),
    ("action-icons/monitor.svg", include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/action-icons/monitor.svg"))),
    ("action-icons/moon.svg", include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/action-icons/moon.svg"))),
    ("action-icons/mouse-left.svg", include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/action-icons/mouse-left.svg"))),
    ("action-icons/mouse-pointer-click.svg", include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/action-icons/mouse-pointer-click.svg"))),
    ("action-icons/mouse-right.svg", include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/action-icons/mouse-right.svg"))),
    ("action-icons/mouse.svg", include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/action-icons/mouse.svg"))),
    ("action-icons/move.svg", include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/action-icons/move.svg"))),
    ("action-icons/palette.svg", include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/action-icons/palette.svg"))),
    ("action-icons/pencil.svg", include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/action-icons/pencil.svg"))),
    ("action-icons/play.svg", include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/action-icons/play.svg"))),
    ("action-icons/redo-2.svg", include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/action-icons/redo-2.svg"))),
    ("action-icons/refresh-cw.svg", include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/action-icons/refresh-cw.svg"))),
    ("action-icons/rotate-ccw.svg", include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/action-icons/rotate-ccw.svg"))),
    ("action-icons/rotate-cw.svg", include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/action-icons/rotate-cw.svg"))),
    ("action-icons/save.svg", include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/action-icons/save.svg"))),
    ("action-icons/scissors.svg", include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/action-icons/scissors.svg"))),
    ("action-icons/scroll-text.svg", include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/action-icons/scroll-text.svg"))),
    ("action-icons/search.svg", include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/action-icons/search.svg"))),
    ("action-icons/settings.svg", include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/action-icons/settings.svg"))),
    ("action-icons/skip-back.svg", include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/action-icons/skip-back.svg"))),
    ("action-icons/skip-forward.svg", include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/action-icons/skip-forward.svg"))),
    ("action-icons/square-arrow-left.svg", include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/action-icons/square-arrow-left.svg"))),
    ("action-icons/square-arrow-right.svg", include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/action-icons/square-arrow-right.svg"))),
    ("action-icons/square-plus.svg", include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/action-icons/square-plus.svg"))),
    ("action-icons/square-terminal.svg", include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/action-icons/square-terminal.svg"))),
    ("action-icons/square-x.svg", include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/action-icons/square-x.svg"))),
    ("action-icons/terminal.svg", include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/action-icons/terminal.svg"))),
    ("action-icons/star.svg", include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/action-icons/star.svg"))),
    ("action-icons/undo-2.svg", include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/action-icons/undo-2.svg"))),
    ("action-icons/unifying.svg", include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/action-icons/unifying.svg"))),
    ("action-icons/usb.svg", include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/action-icons/usb.svg"))),
    ("action-icons/user.svg", include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/action-icons/user.svg"))),
    ("action-icons/volume-1.svg", include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/action-icons/volume-1.svg"))),
    ("action-icons/volume-2.svg", include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/action-icons/volume-2.svg"))),
    ("action-icons/volume-x.svg", include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/action-icons/volume-x.svg"))),
    ("action-icons/x.svg", include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/action-icons/x.svg"))),
];

/// The embedded SVG for an `action-icons/…` asset path — the same path
/// `ActionRingIcon::asset_path` names — or `None` for any other path.
#[must_use]
pub fn embedded_icon(path: &str) -> Option<&'static [u8]> {
    ACTION_ICONS
        .iter()
        .find(|(candidate, _)| *candidate == path)
        .map(|(_, bytes)| *bytes)
}

/// Every embedded asset path.
pub fn embedded_icon_paths() -> impl Iterator<Item = &'static str> {
    ACTION_ICONS.iter().map(|(path, _)| *path)
}

#[cfg(test)]
mod tests {
    use openlogi_core::binding::ActionRingIcon;

    use super::*;

    #[test]
    fn every_ring_gallery_icon_is_embedded() {
        for icon in ActionRingIcon::ALL {
            assert!(
                embedded_icon(icon.asset_path()).is_some(),
                "missing embedded asset for {icon:?}"
            );
        }
    }

    /// On-disk but un-embedded icons fail exactly the way the GPUI loader
    /// does: the asset source returns `None`, gpui falls through to the next
    /// source, and the renderer gets `could not find asset` plus a blank icon.
    /// So the whole vendored directory must be covered, not just the paths any
    /// one code path references today.
    #[test]
    fn every_vendored_icon_is_embedded() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("action-icons");
        let mut count = 0;
        for entry in std::fs::read_dir(dir).expect("vendored icon directory") {
            let name = entry.expect("directory entry").path();
            let asset = format!(
                "action-icons/{}",
                name.file_name().expect("icon file name").to_string_lossy()
            );
            assert!(
                embedded_icon(&asset).is_some(),
                "{asset} exists on disk but is not embedded"
            );
            count += 1;
        }
        assert!(count > 0, "no icons found on disk");
    }
}
