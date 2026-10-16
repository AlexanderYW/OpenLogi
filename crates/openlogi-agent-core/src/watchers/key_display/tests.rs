use openlogi_core::binding::{ActionRingIcon, GridKey};
use openlogi_device_registry::keypad::{KeyGrid, PanelPoint};

use super::*;

const DISPLAY: KeyDisplay = KeyDisplay {
    key_size: 118,
    origin: PanelPoint { x: 23, y: 6 },
    pitch: 158,
    grid: KeyGrid { rows: 3, cols: 3 },
};

fn keypad(product_id: u16) -> DeviceRoute {
    DeviceRoute::Direct {
        vendor_id: 0x046d,
        product_id,
    }
}

fn spec(route: DeviceRoute, generation: u64, copy_on: &[GridKey]) -> KeyDisplaySpec {
    KeyDisplaySpec {
        route,
        display: DISPLAY,
        generation,
        faces: GridKey::ALL
            .into_iter()
            .map(|key| {
                let face = if copy_on.contains(&key) {
                    KeyFace::Icon(ActionRingIcon::Copy)
                } else {
                    KeyFace::Blank
                };
                (key, face)
            })
            .collect(),
    }
}

fn keys(paints: &[Paint]) -> Vec<GridKey> {
    paints.iter().map(|paint| paint.key).collect()
}

fn record_all(painted: &mut Painted, paints: &[Paint]) {
    for paint in paints {
        painted.record(paint);
    }
}

#[test]
fn a_new_panel_is_painted_in_full() {
    let mut painted = Painted::default();
    let specs = [spec(keypad(0xc354), 0, &[GridKey::Middle])];
    let plan = painted.plan(&specs);

    assert_eq!(keys(&plan), GridKey::ALL);
    let middle = plan
        .iter()
        .find(|paint| paint.key == GridKey::Middle)
        .unwrap();
    assert_eq!(middle.face, KeyFace::Icon(ActionRingIcon::Copy));
}

#[test]
fn only_keys_whose_face_changed_are_repainted() {
    let mut painted = Painted::default();
    let specs = [spec(keypad(0xc354), 0, &[GridKey::Middle])];
    let first = painted.plan(&specs);
    record_all(&mut painted, &first);

    assert!(painted.plan(&specs).is_empty());
    let rebound = [spec(keypad(0xc354), 0, &[GridKey::TopLeft])];
    assert_eq!(
        keys(&painted.plan(&rebound)),
        [GridKey::TopLeft, GridKey::Middle]
    );
}

#[test]
fn an_unacknowledged_key_stays_pending() {
    let mut painted = Painted::default();
    let specs = [spec(keypad(0xc354), 0, &[])];
    let first = painted.plan(&specs);
    // Only the first key's write was acknowledged.
    painted.record(&first[0]);

    assert_eq!(keys(&painted.plan(&specs)), GridKey::ALL[1..]);
}

#[test]
fn a_moved_generation_repaints_the_whole_panel() {
    let mut painted = Painted::default();
    let specs = [spec(keypad(0xc354), 4, &[GridKey::Middle])];
    let first = painted.plan(&specs);
    record_all(&mut painted, &first);

    let after_reconnect = [spec(keypad(0xc354), 5, &[GridKey::Middle])];
    assert_eq!(keys(&painted.plan(&after_reconnect)), GridKey::ALL);
}

#[test]
fn a_panel_that_left_and_returned_is_repainted_in_full() {
    let mut painted = Painted::default();
    let specs = [spec(keypad(0xc354), 0, &[])];
    let first = painted.plan(&specs);
    record_all(&mut painted, &first);

    assert!(painted.plan(&[]).is_empty());
    assert_eq!(keys(&painted.plan(&specs)), GridKey::ALL);
}

#[test]
fn an_acknowledgement_from_before_a_reset_is_not_trusted() {
    let mut painted = Painted::default();
    let before = [spec(keypad(0xc354), 1, &[])];
    let stale = painted.plan(&before);
    // The panel reset (generation 2) while generation 1's writes were out.
    let after = [spec(keypad(0xc354), 2, &[])];
    let current = painted.plan(&after);
    painted.record(&current[0]);
    record_all(&mut painted, &stale);

    assert_eq!(keys(&painted.plan(&after)), GridKey::ALL[1..]);
}

#[test]
fn panels_are_tracked_independently() {
    let mut painted = Painted::default();
    let both = [
        spec(keypad(0xc354), 0, &[]),
        spec(keypad(0xc355), 0, &[GridKey::BottomRight]),
    ];
    let first = painted.plan(&both);
    assert_eq!(first.len(), 18);
    record_all(&mut painted, &first);

    let rebound = [
        spec(keypad(0xc354), 0, &[GridKey::TopRight]),
        spec(keypad(0xc355), 0, &[GridKey::BottomRight]),
    ];
    let changed = painted.plan(&rebound);
    assert_eq!(changed.len(), 1);
    assert_eq!(changed[0].spec.route, keypad(0xc354));
    assert_eq!(changed[0].key, GridKey::TopRight);
}

#[test]
fn identical_faces_render_once() {
    let mut tiles = Tiles::default();
    let face = KeyFace::Icon(ActionRingIcon::Copy);
    let first = tiles.get(face, 118).unwrap();
    let second = tiles.get(face, 118).unwrap();
    assert!(Arc::ptr_eq(&first, &second));
    assert!(!Arc::ptr_eq(
        &first,
        &tiles.get(KeyFace::Blank, 118).unwrap()
    ));
}

#[test]
fn a_keypad_that_keeps_failing_is_reported_once_until_it_recovers() {
    let mut painted = Painted::default();
    let pad = keypad(0xc354);
    let specs = [spec(pad.clone(), 0, &[])];

    assert!(painted.failed(&pad));
    assert!(!painted.failed(&pad));
    // Another keypad's first failure is its own.
    assert!(painted.failed(&keypad(0xc355)));

    let plan = painted.plan(&specs);
    painted.record(&plan[0]);
    assert!(painted.failed(&pad));
}

#[test]
fn a_keypad_that_leaves_and_returns_is_reported_afresh() {
    let mut painted = Painted::default();
    let pad = keypad(0xc354);
    assert!(painted.failed(&pad));

    painted.plan(&[spec(pad.clone(), 0, &[])]);
    assert!(!painted.failed(&pad));

    painted.plan(&[]);
    assert!(painted.failed(&pad));
}

#[test]
fn an_acknowledged_write_ends_the_retries() {
    let mut painted = Painted::default();
    let pad = keypad(0xc354);
    let specs = [spec(pad.clone(), 0, &[])];
    painted.failed(&pad);
    assert!(painted.is_failing());

    let plan = painted.plan(&specs);
    painted.record(&plan[0]);

    assert!(!painted.is_failing());
}
