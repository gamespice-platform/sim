//! GSBotIntentTable's finish (gs-bot API 52): per row, the stick limited to length 1 and the
//! command buttons through a game's bit maps, as the script twin computes them, to the bit.

use crate::num::Vec2;

/// Intent bits a map has an entry for (fire, crouch, jump, walk, air jump, throw, use, stranded).
pub const BITS: usize = 8;

/// One row: `move.limit_length(1.0)`, then the OR of `move_map` and `fire_map` over the set bits
/// of `buttons` (a missing map entry counts as 0).
#[inline]
pub fn row(stick: Vec2, buttons: i32, move_map: &[i32], fire_map: &[i32]) -> (Vec2, i32, i32) {
    let (mut mb, mut fb) = (0, 0);
    for k in 0..BITS {
        if buttons & (1 << k) != 0 {
            mb |= move_map.get(k).copied().unwrap_or(0);
            fb |= fire_map.get(k).copied().unwrap_or(0);
        }
    }
    (stick.limit_length(1.0), mb, fb)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stick_is_limited_and_short_sticks_kept() {
        let (m, _, _) = row(Vec2::new(3.0, 4.0), 0, &[], &[]);
        assert_eq!(
            (m.x.to_bits(), m.y.to_bits()),
            (0.6f32.to_bits(), 0.8f32.to_bits())
        );
        let short = Vec2::new(0.3, -0.2);
        let (m, _, _) = row(short, 0, &[], &[]);
        assert_eq!((m.x, m.y), (short.x, short.y));
        let (m, _, _) = row(Vec2::new(0.0, 0.0), 0, &[], &[]);
        assert!(m.is_zero());
    }

    #[test]
    fn buttons_follow_the_maps() {
        let move_map = [0, 2, 1, 8, 32, 0, 0, 0];
        let fire_map = [1, 0, 0, 0, 0, 0, 8, 0];
        assert_eq!(row(Vec2::new(0.0, 0.0), 1 | 8, &move_map, &fire_map).1, 8);
        assert_eq!(row(Vec2::new(0.0, 0.0), 1 | 8, &move_map, &fire_map).2, 1);
        assert_eq!(
            row(Vec2::new(0.0, 0.0), 4 | 64 | 128, &move_map, &fire_map).1,
            1
        );
        assert_eq!(
            row(Vec2::new(0.0, 0.0), 4 | 64 | 128, &move_map, &fire_map).2,
            8
        );
        assert_eq!(row(Vec2::new(0.0, 0.0), 255, &move_map[..2], &[]).1, 2);
    }
}
