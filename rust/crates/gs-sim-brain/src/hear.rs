//! GSBotBrain's `_hear`: which of the sounds made since the brain last listened it hears, and
//! where it thinks the last one came from. The script's arithmetic in its order and precision.

use crate::num::{Stream, Vec3};

/// One packed sound row (GSBotWorld's sound rows): `seq, source, x, y, z, loudness, at`.
pub const STRIDE: usize = 7;
/// GSBotWorld.SOUND_LIFETIME: a sound older than this at the front of the list is gone.
pub const LIFETIME: f64 = 1.0;

/// What `_hear` reads besides the sounds.
#[derive(Clone, Copy, Debug)]
pub struct HearIn {
    /// The newest sound the brain heard last time.
    pub after_seq: i64,
    pub now: f64,
    /// The listener's position.
    pub here: Vec3,
    /// The profile's hearing reach, metres for a loudness of 1.
    pub reach: f64,
    pub my_id: i64,
    pub my_team: i64,
    /// `p.aggression + style.aggression_b`, the chance it goes after the sound.
    pub eager: f64,
}

/// The last sound heard: where the brain guesses it was, and whether it goes after it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Heard {
    pub pos: Vec3,
    pub chase: bool,
}

/// `GSBotContact.is_enemy` from the listener's team to `other`'s.
#[inline]
fn is_enemy(my_team: i64, other: i64) -> bool {
    my_team == 0 || other == 0 || my_team != other
}

/// `_hear`'s loop over `rows` (`STRIDE` each, oldest first). `team_of(id)` is the team of the
/// contact with that id, `None` when there is none (an unknown source is always heard). Draws
/// three units from `rng` per sound heard; returns the last one heard, if any.
pub fn hear(
    rows: &[f64],
    inp: &HearIn,
    team_of: impl Fn(i64) -> Option<i64>,
    rng: &mut Stream,
) -> Option<Heard> {
    let n = rows.len() / STRIDE;
    // GSBotWorld._prune drops expired sounds from the front only.
    let mut first = 0;
    while first < n && inp.now - rows[first * STRIDE + 6] > LIFETIME {
        first += 1;
    }
    let mut out = None;
    for i in first..n {
        let r = &rows[i * STRIDE..(i + 1) * STRIDE];
        if r[0] as i64 <= inp.after_seq {
            continue;
        }
        let source = r[1] as i64;
        if source == inp.my_id {
            continue;
        }
        let at = Vec3::new(r[2] as f32, r[3] as f32, r[4] as f32);
        if at.x == f32::INFINITY && at.y == f32::INFINITY && at.z == f32::INFINITY {
            continue;
        }
        let d = inp.here.distance_to(at) as f64;
        if d > inp.reach * r[5] {
            continue;
        }
        if let Some(team) = team_of(source)
            && !is_enemy(inp.my_team, team)
        {
            continue;
        }
        let spread = d * 0.08;
        let dx = (rng.next_unit() - 0.5) * 2.0 * spread;
        let dz = (rng.next_unit() - 0.5) * 2.0 * spread;
        let pos = at + Vec3::new(dx as f32, 0.0, dz as f32);
        let chase = rng.next_unit() < inp.eager;
        out = Some(Heard { pos, chase });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(seq: i64, source: i64, at: Vec3, loud: f64, t: f64) -> [f64; STRIDE] {
        [
            seq as f64,
            source as f64,
            at.x as f64,
            at.y as f64,
            at.z as f64,
            loud,
            t,
        ]
    }

    fn input() -> HearIn {
        HearIn {
            after_seq: 0,
            now: 5.0,
            here: Vec3::new(0.0, 0.0, 0.0),
            reach: 30.0,
            my_id: 1,
            my_team: 1,
            eager: 0.5,
        }
    }

    #[test]
    fn hears_an_enemy_and_draws_three() {
        let rows = row(1, 2, Vec3::new(10.0, 0.0, 0.0), 1.0, 4.9);
        let mut rng = Stream::new(7, 0);
        let h = hear(&rows, &input(), |_| Some(2), &mut rng).unwrap();
        assert_eq!(rng.counter, 3);
        assert!((h.pos.x - 10.0).abs() <= 0.8 && (h.pos.z).abs() <= 0.8 && h.pos.y == 0.0);
    }

    #[test]
    fn skips_self_friends_far_old_and_seen() {
        let mut rows = Vec::new();
        rows.extend(row(1, 2, Vec3::new(1.0, 0.0, 0.0), 1.0, 3.0)); // expired at the front
        rows.extend(row(2, 1, Vec3::new(1.0, 0.0, 0.0), 1.0, 4.9)); // its own
        rows.extend(row(3, 3, Vec3::new(1.0, 0.0, 0.0), 1.0, 4.9)); // a friend
        rows.extend(row(4, 2, Vec3::new(40.0, 0.0, 0.0), 0.5, 4.9)); // too far for its loudness
        rows.extend(row(5, 2, Vec3::new(1.0, 0.0, 0.0), 1.0, 4.9)); // already heard
        let mut inp = input();
        inp.after_seq = 4;
        let mut rng = Stream::new(7, 0);
        let team = |id: i64| Some(if id == 3 { 1 } else { 2 });
        assert!(hear(&rows[..STRIDE * 4], &input(), team, &mut rng).is_none());
        assert_eq!(rng.counter, 0);
        assert!(hear(&rows, &inp, team, &mut rng).is_some());
        assert_eq!(rng.counter, 3);
    }

    #[test]
    fn unknown_source_and_free_for_all_are_heard() {
        let rows = row(1, 9, Vec3::new(2.0, 0.0, 2.0), 1.0, 5.0);
        let mut rng = Stream::new(3, 10);
        assert!(hear(&rows, &input(), |_| None, &mut rng).is_some());
        let mut inp = input();
        inp.my_team = 0;
        assert!(hear(&rows, &inp, |_| Some(0), &mut rng).is_some());
        assert_eq!(rng.counter, 16);
    }

    #[test]
    fn the_last_heard_wins() {
        let mut rows = Vec::new();
        rows.extend(row(1, 2, Vec3::new(1.0, 0.0, 0.0), 1.0, 5.0));
        rows.extend(row(2, 2, Vec3::new(0.0, 0.0, 20.0), 1.0, 5.0));
        let mut rng = Stream::new(11, 0);
        let h = hear(&rows, &input(), |_| Some(2), &mut rng).unwrap();
        assert!(h.pos.z > 18.0);
        assert_eq!(rng.counter, 6);
    }
}
