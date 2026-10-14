use openlogi_core::binding::{ActionRingIcon, GridKey};

use super::*;

const DISPLAY: KeyDisplay = KeyDisplay {
    key_size: 118,
    origin: (23, 6),
    pitch: 158,
    grid: (3, 3),
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
    let plan = painted.plan(&[spec(keypad(0xc354), 0, &[GridKey::Middle])]);

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
    let first = painted.plan(&[spec(keypad(0xc354), 0, &[GridKey::Middle])]);
    record_all(&mut painted, &first);

    assert!(
        painted
            .plan(&[spec(keypad(0xc354), 0, &[GridKey::Middle])])
            .is_empty()
    );
    let rebound = painted.plan(&[spec(keypad(0xc354), 0, &[GridKey::TopLeft])]);
    assert_eq!(keys(&rebound), [GridKey::TopLeft, GridKey::Middle]);
}

#[test]
fn an_unacknowledged_key_stays_pending() {
    let mut painted = Painted::default();
    let first = painted.plan(&[spec(keypad(0xc354), 0, &[])]);
    // Only the first key's write was acknowledged.
    painted.record(&first[0]);

    let again = painted.plan(&[spec(keypad(0xc354), 0, &[])]);
    assert_eq!(keys(&again), GridKey::ALL[1..]);
}

#[test]
fn a_moved_generation_repaints_the_whole_panel() {
    let mut painted = Painted::default();
    let first = painted.plan(&[spec(keypad(0xc354), 4, &[GridKey::Middle])]);
    record_all(&mut painted, &first);

    let after_reconnect = painted.plan(&[spec(keypad(0xc354), 5, &[GridKey::Middle])]);
    assert_eq!(keys(&after_reconnect), GridKey::ALL);
}

#[test]
fn a_panel_that_left_and_returned_is_repainted_in_full() {
    let mut painted = Painted::default();
    let first = painted.plan(&[spec(keypad(0xc354), 0, &[])]);
    record_all(&mut painted, &first);

    assert!(painted.plan(&[]).is_empty());
    let back = painted.plan(&[spec(keypad(0xc354), 0, &[])]);
    assert_eq!(keys(&back), GridKey::ALL);
}

#[test]
fn an_acknowledgement_from_before_a_reset_is_not_trusted() {
    let mut painted = Painted::default();
    let stale = painted.plan(&[spec(keypad(0xc354), 1, &[])]);
    // The panel reset (generation 2) while generation 1's writes were out.
    let current = painted.plan(&[spec(keypad(0xc354), 2, &[])]);
    painted.record(&current[0]);
    record_all(&mut painted, &stale);

    let again = painted.plan(&[spec(keypad(0xc354), 2, &[])]);
    assert_eq!(keys(&again), GridKey::ALL[1..]);
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

    let changed = painted.plan(&[
        spec(keypad(0xc354), 0, &[GridKey::TopRight]),
        spec(keypad(0xc355), 0, &[GridKey::BottomRight]),
    ]);
    assert_eq!(changed.len(), 1);
    assert_eq!(changed[0].route, keypad(0xc354));
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
