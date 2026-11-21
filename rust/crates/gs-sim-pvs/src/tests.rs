use super::*;

/// Three rooms along x: x < -10 is cluster 0, -10..10 is cluster 1 (a solid leaf above y = 5),
/// x >= 10 is cluster 2. 0 and 2 see 1, not each other.
fn rooms() -> Pvs {
    Pvs {
        planes: vec![
            1.0, 0.0, 0.0, -10.0, // x >= -10
            1.0, 0.0, 0.0, 10.0, // x >= 10
            0.0, 1.0, 0.0, 5.0, // y >= 5
        ],
        // node 0: x >= -10 ? node 1 : leaf 0; node 1: x >= 10 ? leaf 3 : node 2; node 2: y >= 5 ? leaf 2 : leaf 1
        nodes: vec![0, 1, -1, 1, -4, 2, 2, -3, -2],
        head: 0,
        leaf_cluster: vec![0, 1, -1, 2],
        rows: vec![0b011, 0b111, 0b110],
        row_bytes: 1,
        clusters: 3,
        bounds: None,
    }
}

#[test]
fn walks_to_the_leaf_cluster() {
    let p = rooms();
    assert!(p.is_ready());
    assert_eq!(p.cluster_of([-20.0, 0.0, 0.0]), 0);
    assert_eq!(p.cluster_of([0.0, 0.0, 0.0]), 1);
    assert_eq!(p.cluster_of([0.0, 6.0, 0.0]), -1);
    assert_eq!(p.cluster_of([20.0, 0.0, 0.0]), 2);
    // On the plane is in front, as the script's `d >= 0.0`.
    assert_eq!(p.cluster_of([-10.0, 0.0, 0.0]), 1);
    assert_eq!(p.cluster_of([10.0, 0.0, 0.0]), 2);
}

#[test]
fn rows_decide_and_unknowns_answer_visible() {
    let p = rooms();
    assert!(p.visible([-20.0, 0.0, 0.0], [0.0, 0.0, 0.0]));
    assert!(!p.visible([-20.0, 0.0, 0.0], [20.0, 0.0, 0.0]));
    assert!(!p.visible([20.0, 0.0, 0.0], [-20.0, 0.0, 0.0]));
    assert!(
        p.visible([-20.0, 0.0, 0.0], [0.0, 6.0, 0.0]),
        "solid answers visible"
    );
    assert!(p.cluster_sees(-1, 2) && p.cluster_sees(0, 3) && p.cluster_sees(7, 0));
}

#[test]
fn bounds_as_the_script_tests_them() {
    let mut p = rooms();
    p.bounds = bounds_of([-30.0, -5.0, -5.0], [60.0, 10.0, 10.0]);
    assert_eq!(p.cluster_of([-20.0, 0.0, 0.0]), 0);
    assert_eq!(p.cluster_of([-31.0, 0.0, 0.0]), -1);
    assert_eq!(p.cluster_of([30.0, 5.0, 5.0]), 2, "the end is inside");
    assert!(p.visible([-31.0, 0.0, 0.0], [20.0, 0.0, 0.0]));
    assert_eq!(
        bounds_of([0.0; 3], [1.0, 0.0, 1.0]),
        None,
        "no volume, no bound"
    );
}

#[test]
fn unusable_sets_answer_visible() {
    let mut p = rooms();
    p.row_bytes = 2;
    assert!(!p.is_ready());
    assert!(p.visible([-20.0, 0.0, 0.0], [20.0, 0.0, 0.0]));
    assert!(Pvs::default().visible([0.0; 3], [1.0; 3]));
}

#[test]
fn broken_trees_answer_no_cluster() {
    let mut p = rooms();
    p.nodes = vec![0, 0, 0];
    assert_eq!(p.cluster_of([0.0; 3]), -1, "a loop stops at the guard");
    p.nodes = vec![9, -1, -1];
    assert_eq!(p.cluster_of([0.0; 3]), -1, "a plane past the table");
    p.nodes = vec![0, -9, -9];
    assert_eq!(p.cluster_of([0.0; 3]), -1, "a leaf past the table");
}

#[test]
fn plane_test_is_f64_as_the_script() {
    // 0.1 * 3 in f64 differs from f32; a point exactly where the two disagree goes the script's way.
    let p = Pvs {
        planes: vec![0.1, 0.0, 0.0, 0.3],
        nodes: vec![0, -1, -2],
        head: 0,
        leaf_cluster: vec![0, 1],
        rows: vec![0b11, 0b11],
        row_bytes: 1,
        clusters: 2,
        bounds: None,
    };
    let x = 3.0f32;
    let d = f64::from(0.1f32) * f64::from(x) - f64::from(0.3f32);
    assert_eq!(p.cluster_of([x, 0.0, 0.0]), if d >= 0.0 { 0 } else { 1 });
}
