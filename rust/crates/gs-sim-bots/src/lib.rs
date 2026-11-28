//! Bot brain kernels: pure Rust over plain arrays, no engine types.
//!
//! The first kernel is the sight scan of a bot's think: which bodies are inside its range and
//! view cone, with their closeness score. It is the brain's `_candidates`, the same arithmetic in
//! the same precision, so the brain's choices do not change with the kernel.

pub use gs_sim_math::Vec3;
use gs_sim_math::rad_to_deg;

pub mod choose;
pub mod coast;
pub mod threat;
pub mod tier;

/// Beyond this many degrees off the view, a body already being fought is no longer tracked.
pub const TRACK_DEG: f64 = 90.0;

/// Degrees off the view that double a body's score (its distance).
pub const SCORE_DEG: f64 = 60.0;

/// The bodies of one tick, packed once for every bot that thinks in it.
#[derive(Default, Debug, Clone)]
pub struct Contacts {
    pub ids: Vec<i64>,
    pub points: Vec<Vec3>,
    pub teams: Vec<i32>,
    pub alive: Vec<bool>,
}

impl Contacts {
    /// Number of bodies.
    pub fn len(&self) -> usize {
        self.ids.len()
    }

    /// Whether there are none.
    pub fn is_empty(&self) -> bool {
        self.ids.is_empty()
    }
}

/// One bot's question: who it is, where it looks from and along, and how far and wide it sees.
#[derive(Debug, Clone, Copy)]
pub struct Query {
    pub me_id: i64,
    pub me_team: i32,
    pub eye: Vec3,
    pub forward: Vec3,
    pub half_fov_deg: f64,
    pub sight: f64,
    pub close_notice: f64,
    pub target_id: i64,
    pub visible: bool,
}

/// The scan's answer: candidate indices into [`Contacts`] in order, their scores, and whether
/// the visible target was found dead (the brain then forgets it).
#[derive(Default, Debug, Clone)]
pub struct Found {
    pub index: Vec<i32>,
    pub scores: Vec<f32>,
    pub target_dead: bool,
}

/// Fills `out` with the bodies `q` could be looking at, as the script's `_candidates` does.
pub fn candidates(c: &Contacts, q: &Query, out: &mut Found) {
    out.index.clear();
    out.scores.clear();
    out.target_dead = false;
    let mut visible = q.visible;
    for i in 0..c.len() {
        scan_row(c, q, i, &mut visible, out);
    }
}

/// [`candidates`] over the rows `grid` puts within reach of `q` (plus the target's row), the same
/// answer in the same table order: a row the grid leaves out is beyond sight in any case.
/// `rows` is scratch. Falls back to the whole table when the grid is off or the query's reach
/// covers it.
pub fn candidates_grid(c: &Contacts, grid: &Grid, q: &Query, out: &mut Found, rows: &mut Vec<u32>) {
    if grid.rows.len() != c.len() || !grid.gather(q.eye, q.sight, rows) {
        candidates(c, q, out);
        return;
    }
    if q.visible
        && let Some(t) = c.ids.iter().position(|&id| id == q.target_id)
    {
        rows.push(t as u32);
    }
    rows.sort_unstable();
    rows.dedup();
    out.index.clear();
    out.scores.clear();
    out.target_dead = false;
    let mut visible = q.visible;
    for &i in rows.iter() {
        scan_row(c, q, i as usize, &mut visible, out);
    }
}

/// One row of the scan: the dead-target check, then range, cone and score.
#[inline]
fn scan_row(c: &Contacts, q: &Query, i: usize, visible: &mut bool, out: &mut Found) {
    let id = c.ids[i];
    if !c.alive[i] && id == q.target_id && *visible {
        out.target_dead = true;
        *visible = false;
        return;
    }
    if !c.alive[i] || !is_enemy(q.me_id, q.me_team, id, c.teams[i]) {
        return;
    }
    let to = c.points[i] - q.eye;
    let d = to.length() as f64;
    if d > q.sight {
        return;
    }
    let off = if d > 0.01 {
        rad_to_deg(q.forward.angle_to(to) as f64)
    } else {
        0.0
    };
    let tracking = *visible && id == q.target_id && off < TRACK_DEG;
    if off > q.half_fov_deg && d > q.close_notice && !tracking {
        return;
    }
    out.index.push(i as i32);
    out.scores.push((d * (1.0 + off / SCORE_DEG)) as f32);
}

/// The tick's bodies bucketed by square XZ cells, so a scan visits only the cells within its
/// sight. Built once per tick; off (empty) when the cell size is 0 or a point is not finite.
#[derive(Default, Debug, Clone)]
pub struct Grid {
    cell: f32,
    x0: i32,
    z0: i32,
    w: i32,
    h: i32,
    /// Per cell, where its rows start in `rows` (one extra end entry).
    start: Vec<u32>,
    /// Table rows, cell by cell, ascending within a cell.
    rows: Vec<u32>,
}

impl Grid {
    /// Buckets `points` into cells of `cell` m; an empty grid (off) for `cell <= 0`.
    pub fn build(&mut self, points: &[Vec3], cell: f32) {
        self.start.clear();
        self.rows.clear();
        self.w = 0;
        self.h = 0;
        self.cell = cell;
        if cell <= 0.0 || !cell.is_finite() || points.is_empty() {
            return;
        }
        if points.iter().any(|p| !p.x.is_finite() || !p.z.is_finite()) {
            return;
        }
        let key = |v: f32| (v / cell).floor() as i32;
        let (mut x0, mut x1, mut z0, mut z1) = (i32::MAX, i32::MIN, i32::MAX, i32::MIN);
        for p in points {
            let (x, z) = (key(p.x), key(p.z));
            x0 = x0.min(x);
            x1 = x1.max(x);
            z0 = z0.min(z);
            z1 = z1.max(z);
        }
        let (w, h) = (x1 as i64 - x0 as i64 + 1, z1 as i64 - z0 as i64 + 1);
        if w * h > 1 << 16 {
            return; // a sparse spread: the table is scanned whole
        }
        let (w, h) = (w as i32, h as i32);
        let cell_of = |p: &Vec3| ((key(p.z) - z0) * w + (key(p.x) - x0)) as usize;
        let mut start = vec![0u32; (w * h) as usize + 1];
        for p in points {
            start[cell_of(p) + 1] += 1;
        }
        for k in 1..start.len() {
            start[k] += start[k - 1];
        }
        let mut fill = start.clone();
        self.rows.resize(points.len(), 0);
        for (i, p) in points.iter().enumerate() {
            let c = cell_of(p);
            self.rows[fill[c] as usize] = i as u32;
            fill[c] += 1;
        }
        self.start = start;
        (self.x0, self.z0, self.w, self.h) = (x0, z0, w, h);
    }

    /// The rows in the cells within `reach` m of `eye` (one cell of margin), unordered, into
    /// `out`; false when the grid is off or the reach covers every cell (scan the table).
    pub fn gather(&self, eye: Vec3, reach: f64, out: &mut Vec<u32>) -> bool {
        out.clear();
        if self.w == 0 || !reach.is_finite() || !eye.x.is_finite() || !eye.z.is_finite() {
            return false;
        }
        let cell = self.cell as f64;
        let span = |c: f32, o: i32, n: i32| {
            let lo = ((c as f64 - reach) / cell).floor() as i64 - 1 - o as i64;
            let hi = ((c as f64 + reach) / cell).floor() as i64 + 1 - o as i64;
            (lo.max(0), hi.min(n as i64 - 1))
        };
        let (xa, xb) = span(eye.x, self.x0, self.w);
        let (za, zb) = span(eye.z, self.z0, self.h);
        if xa > xb || za > zb {
            return true; // nobody within reach
        }
        if (xb - xa + 1) * (zb - za + 1) >= self.w as i64 * self.h as i64 {
            return false;
        }
        for z in za..=zb {
            let base = z * self.w as i64;
            let (a, b) = (
                self.start[(base + xa) as usize],
                self.start[(base + xb + 1) as usize],
            );
            out.extend_from_slice(&self.rows[a as usize..b as usize]);
        }
        true
    }

    /// Whether the grid is built (cell size above 0 and a bounded spread).
    pub fn is_on(&self) -> bool {
        self.w > 0
    }
}

/// Whether body `id` of `team` is somebody `me_id` of `me_team` fights; team 0 fights everybody.
#[inline]
pub fn is_enemy(me_id: i64, me_team: i32, id: i64, team: i32) -> bool {
    id != me_id && (me_team == 0 || team == 0 || me_team != team)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn world() -> Contacts {
        let mut c = Contacts::default();
        let rows = [
            (1, Vec3::new(0.0, 1.25, 0.0), 1, true),   // the bot itself
            (2, Vec3::new(0.0, 1.25, -10.0), 2, true), // straight ahead
            (3, Vec3::new(10.0, 1.25, -1.0), 2, true), // 84 degrees off
            (4, Vec3::new(0.0, 1.25, -5.0), 1, true),  // a friend
            (5, Vec3::new(1.0, 1.25, 1.0), 2, true),   // behind, but touching
            (6, Vec3::new(0.0, 1.25, -90.0), 2, true), // out of sight range
            (7, Vec3::new(0.5, 1.25, -8.0), 2, false), // dead
        ];
        for (id, p, team, alive) in rows {
            c.ids.push(id);
            c.points.push(p);
            c.teams.push(team);
            c.alive.push(alive);
        }
        c
    }

    fn query() -> Query {
        Query {
            me_id: 1,
            me_team: 1,
            eye: Vec3::new(0.0, 1.6, 0.0),
            forward: Vec3::new(0.0, 0.0, -1.0),
            half_fov_deg: 50.0,
            sight: 60.0,
            close_notice: 2.0,
            target_id: -1,
            visible: false,
        }
    }

    #[test]
    fn scan_keeps_enemies_in_cone_and_close_ones() {
        let mut f = Found::default();
        candidates(&world(), &query(), &mut f);
        assert_eq!(f.index, vec![1, 4]);
        assert!(!f.target_dead);
        assert!(f.scores[0] > 10.0 && f.scores[0] < 10.5);
    }

    #[test]
    fn a_visible_target_is_tracked_outside_the_cone_below_ninety_degrees() {
        let mut q = query();
        q.target_id = 3;
        q.visible = true;
        let mut f = Found::default();
        candidates(&world(), &q, &mut f);
        assert!(f.index.contains(&2), "{:?}", f.index);
    }

    #[test]
    fn a_dead_visible_target_is_reported_and_skipped() {
        let mut q = query();
        q.target_id = 7;
        q.visible = true;
        let mut f = Found::default();
        candidates(&world(), &q, &mut f);
        assert!(f.target_dead);
        assert!(!f.index.contains(&6));
    }

    #[test]
    fn team_zero_fights_everybody_but_itself() {
        assert!(is_enemy(1, 0, 2, 1));
        assert!(is_enemy(1, 1, 2, 0));
        assert!(!is_enemy(1, 1, 2, 1));
        assert!(!is_enemy(1, 0, 1, 0));
    }

    fn spread() -> Contacts {
        let mut c = Contacts::default();
        let mut seed = 12345u64;
        let mut rnd = || {
            seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            ((seed >> 33) as f32 / (1u64 << 31) as f32) * 2.0 - 1.0
        };
        for i in 0..64 {
            c.ids.push(i + 1);
            c.points
                .push(Vec3::new(rnd() * 150.0, 1.25 + rnd(), rnd() * 150.0));
            c.teams.push(if i % 2 == 0 { 1 } else { 2 });
            c.alive.push(i % 7 != 3);
        }
        c
    }

    #[test]
    fn the_grid_scan_is_the_table_scan() {
        let c = spread();
        let mut rows = Vec::new();
        for cell in [4.0f32, 16.0, 40.0] {
            let mut g = Grid::default();
            g.build(&c.points, cell);
            assert!(g.is_on());
            for me in 0..c.len() {
                for (sight, target, vis) in [(20.0, -1, false), (60.0, 4, true), (500.0, 11, true)]
                {
                    let mut q = query();
                    q.me_id = c.ids[me];
                    q.me_team = c.teams[me];
                    q.eye = c.points[me] + Vec3::new(0.0, 0.35, 0.0);
                    q.forward = Vec3::new((me as f32).sin(), 0.0, (me as f32).cos());
                    q.sight = sight;
                    q.target_id = target;
                    q.visible = vis;
                    let (mut a, mut b) = (Found::default(), Found::default());
                    candidates(&c, &q, &mut a);
                    candidates_grid(&c, &g, &q, &mut b, &mut rows);
                    assert_eq!(a.index, b.index, "cell {cell} me {me} sight {sight}");
                    assert_eq!(a.scores, b.scores);
                    assert_eq!(a.target_dead, b.target_dead);
                }
            }
        }
    }

    #[test]
    fn an_off_grid_scans_the_table() {
        let c = spread();
        let mut g = Grid::default();
        g.build(&c.points, 0.0);
        assert!(!g.is_on());
        let mut rows = Vec::new();
        assert!(!g.gather(Vec3::new(0.0, 0.0, 0.0), 10.0, &mut rows));
    }
}
