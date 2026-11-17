use super::*;

fn rect(x: f32, y: f32, w: f32, h: f32) -> Rect {
    Rect { x, y, w, h }
}

fn ids_in(g: &Grid, r: Rect, exclude: i64) -> Vec<i64> {
    let mut out = Vec::new();
    g.query_rect(&r, exclude, &mut out);
    out
}

#[test]
fn keys_pack_like_the_script() {
    assert_eq!(key(0, 0), 0);
    assert_eq!(key(1, 2), (1 << 32) | 2);
    assert_eq!(key(-1, -1), -1);
    assert_eq!(key(-1, 0), 0xFFFF_FFFF_i64 << 32);
    assert_eq!(cell_of(-0.5, 128.0), -1);
    assert_eq!(cell_of(127.99, 128.0), 0);
    assert_eq!(cell_of(128.0, 128.0), 1);
}

#[test]
fn swap_and_pop_order_and_empty_cells_drop() {
    let mut g = Grid::new(100.0);
    for id in 1..=4 {
        g.place(id, 10.0 * id as f32, 10.0, 0.0);
    }
    assert_eq!(g.bucket(key(0, 0)), &[1, 2, 3, 4]);
    g.remove(2);
    assert_eq!(g.bucket(key(0, 0)), &[1, 4, 3]);
    // A move within the cell keeps the order; a move out swap-and-pops.
    g.place(1, 50.0, 50.0, 0.0);
    assert_eq!(g.bucket(key(0, 0)), &[1, 4, 3]);
    g.place(1, 150.0, 50.0, 0.0);
    assert_eq!(g.bucket(key(0, 0)), &[3, 4]);
    assert_eq!(g.bucket(key(1, 0)), &[1]);
    g.remove(1);
    assert_eq!(g.cell_count(), 1);
    assert_eq!(g.size(), 2);
    assert_eq!(g.position_of(1), (0.0, 0.0));
}

#[test]
fn rect_walks_x_then_y_and_excludes() {
    let mut g = Grid::new(10.0);
    g.place(1, 15.0, 5.0, 0.0);
    g.place(2, 5.0, 15.0, 0.0);
    g.place(3, 5.0, 5.0, 0.0);
    g.place(4, 15.0, 15.0, 0.0);
    // Cells (0,0), (0,1), (1,0), (1,1).
    assert_eq!(ids_in(&g, rect(0.0, 0.0, 19.0, 19.0), 0), vec![3, 2, 1, 4]);
    assert_eq!(ids_in(&g, rect(0.0, 0.0, 19.0, 19.0), 2), vec![3, 1, 4]);
    // The end edges are outside.
    assert_eq!(ids_in(&g, rect(0.0, 0.0, 15.0, 15.0), 0), vec![3]);
}

#[test]
fn forage_scores_like_the_bot() {
    let rules = ForageRules::default();
    let mut g = Grid::new(128.0);
    g.place(7, 100.0, 0.0, 0.0); // a piece
    g.place(1_000_001, 50.0, 0.0, 2.0); // food worth 2 at 50: 2/90
    g.place(1_000_002, 0.0, 30.0, 2.5); // 2.5/70 is more
    g.place(3_000_000, 0.0, -20.0, 0.0); // a pickup: 220/60 wins when allowed
    g.place(1_000_003, 0.0, 0.0, 99.0); // at the origin: skipped
    g.place(5, 0.0, 0.0, 0.0); // the bot's own head, excluded
    let open = g.forage(&rules, (0.0, 0.0), 900.0, 5, true);
    assert_eq!(open.snack_score, 220.0 / 60.0);
    assert_eq!((open.snack_x, open.snack_y), (0.0, -20.0));
    assert_eq!(
        open.rivals,
        vec![Rival {
            id: 7,
            x: 100.0,
            y: 0.0,
            distance: 100.0
        }]
    );
    let full = g.forage(&rules, (0.0, 0.0), 900.0, 5, false);
    assert_eq!(full.snack_score, 2.5 / 70.0);
    assert_eq!((full.snack_x, full.snack_y), (0.0, 30.0));
}

#[test]
fn forage_keeps_the_first_of_equals_and_sees_nothing_far() {
    let rules = ForageRules::default();
    let mut g = Grid::new(64.0);
    g.place(1_000_001, 30.0, 0.0, 1.0);
    g.place(1_000_002, 0.0, 30.0, 1.0);
    let f = g.forage(&rules, (0.0, 0.0), 100.0, 0, true);
    let order = ids_in(&g, rect(-100.0, -100.0, 200.0, 200.0), 0);
    assert_eq!(g.position_of(order[0]), (f.snack_x, f.snack_y));
    let none = g.forage(&rules, (5000.0, 5000.0), 100.0, 0, true);
    assert_eq!(none.snack_score, f64::NEG_INFINITY);
    assert!(none.rivals.is_empty());
}

#[test]
fn adopt_restores_a_cell_exactly() {
    let mut g = Grid::new(10.0);
    g.adopt_cell(key(0, 0), &[(3, 1.0, 1.0, 0.5), (1, 2.0, 2.0, 0.25)]);
    assert_eq!(g.bucket(key(0, 0)), &[3, 1]);
    assert_eq!(g.worth_of(1), 0.25);
    g.remove(3);
    assert_eq!(g.bucket(key(0, 0)), &[1]);
}
