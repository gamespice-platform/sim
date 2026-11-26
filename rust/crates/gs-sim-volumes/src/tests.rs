use super::*;

// Grown 1 under the low corner on every axis, 1/2/1 over the high one.
const BELOW: [f32; 3] = [1.0, 1.0, 1.0];
const ABOVE: [f32; 3] = [1.0, 2.0, 1.0];

fn set() -> VolumeSet {
    let mut s = VolumeSet::default();
    s.add(Bounds::from_position_size(
        [10.0, 0.0, 10.0],
        [2.0, 2.0, 2.0],
    ));
    // A negative size, as an importer's corners can give: flipped to [-12, -10] on x.
    s.add(Bounds::from_position_size(
        [-10.0, 0.0, 0.0],
        [-2.0, 1.0, 1.0],
    ));
    s
}

#[test]
fn empty_set_is_clear_everywhere() {
    let s = VolumeSet::default();
    assert!(s.is_empty());
    assert_eq!(
        s.clear_of(&[[0.0; 3], [1e9, -1e9, 3.0]], BELOW, ABOVE),
        vec![1, 1]
    );
}

#[test]
fn inside_and_far() {
    let s = set();
    assert_eq!(s.len(), 2);
    let pts = [
        [11.0, 1.0, 11.0],
        [0.0, 0.0, 0.0],
        [-11.0, 0.5, 0.5],
        [50.0, 0.0, 50.0],
    ];
    assert_eq!(s.clear_of(&pts, BELOW, ABOVE), vec![0, 1, 0, 1]);
}

#[test]
fn flipped_box_matches_abs() {
    let b = Bounds::from_position_size([-10.0, 0.0, 0.0], [-2.0, 1.0, 1.0]);
    assert_eq!(b.position, [-12.0, 0.0, 0.0]);
    assert_eq!(b.size, [2.0, 1.0, 1.0]);
}

#[test]
fn meeting_a_face_counts_as_near() {
    let s = set();
    // The low face grown by 1: x = 9.0 exactly is near.
    assert_eq!(s.clear_of(&[[9.0, 1.0, 11.0]], BELOW, ABOVE), vec![0]);
    // One ulp short of it is clear.
    let x = f32::from_bits(9.0f32.to_bits() - 1);
    assert_eq!(s.clear_of(&[[x, 1.0, 11.0]], BELOW, ABOVE), vec![1]);
}

#[test]
fn reach_is_asymmetric_on_y() {
    let s = set();
    // Top 2.0 grown by 2 reaches 4.0; bottom 0.0 grown by 1 reaches -1.0.
    assert_eq!(
        s.clear_of(
            &[
                [11.0, 4.0, 11.0],
                [11.0, 4.5, 11.0],
                [11.0, -1.0, 11.0],
                [11.0, -1.5, 11.0]
            ],
            BELOW,
            ABOVE
        ),
        vec![0, 1, 0, 1]
    );
}

#[test]
fn nan_is_never_clear() {
    let s = VolumeSet::default();
    assert_eq!(
        s.clear_of(
            &[[f32::NAN, 0.0, 0.0], [0.0, f32::INFINITY, 0.0]],
            BELOW,
            ABOVE
        ),
        vec![0, 0]
    );
}

#[test]
fn clear_resets() {
    let mut s = set();
    s.clear();
    assert!(s.is_empty());
    assert_eq!(s.clear_of(&[[11.0, 1.0, 11.0]], BELOW, ABOVE), vec![1]);
}
