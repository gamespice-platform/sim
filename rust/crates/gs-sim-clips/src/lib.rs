//! A figure's skeletal clip playback, pure Rust.
//!
//! The twin of the subset of Godot 4.7's `AnimationPlayer` a humanoid figure drives (`play` with a
//! cross-fade, `seek`, `advance`, `speed_scale`, `current_animation*`) together with
//! `AnimationMixer`'s non-deterministic blend of position and rotation tracks into a skeleton's
//! bone poses. Clips are baked once per library into flat key arrays ([`ClipSet`]); a [`Player`]
//! per figure keeps the playback state and leaves, after each call, the bone poses to write.
//!
//! Every step follows the engine's code and float widths: `Animation::_find` and
//! `Animation::_interpolate` (forward, `LOOP_NONE` and `LOOP_LINEAR`, nearest and linear keys),
//! `AnimationPlayer::_process_playback_data` and `_blend_playback_data` (the blend list, the end of
//! a one-shot clip), and the mixer's `_blend_init`, `_blend_calc_total_weight`, `_blend_process` and
//! `_blend_apply` (a bone is written only when its total weight is not zero; its position only when
//! some clip has a position track for it, likewise rotation).

use gs_sim_math::{Quat, lerp_f32, quat_mul, quat_normalized, quat_slerp};

/// `CMP_EPSILON`.
const EPS: f64 = 0.00001;

/// A position, three floats.
pub type Vec3 = [f32; 3];

/// `Math::is_equal_approx(double, double)`.
fn eq_approx(a: f64, b: f64) -> bool {
    if a == b {
        return true;
    }
    let tolerance = (EPS * a.abs()).max(EPS);
    (a - b).abs() < tolerance
}

fn less_approx(a: f64, b: f64) -> bool {
    a < b && !eq_approx(a, b)
}

fn greater_approx(a: f64, b: f64) -> bool {
    a > b && !eq_approx(a, b)
}

fn less_or_eq_approx(a: f64, b: f64) -> bool {
    a < b || eq_approx(a, b)
}

fn greater_or_eq_approx(a: f64, b: f64) -> bool {
    a > b || eq_approx(a, b)
}

/// `Math::is_zero_approx(float)`.
fn zero_approx_f32(v: f32) -> bool {
    v.abs() < EPS as f32
}

/// `Math::fposmod(double, double)`.
fn fposmod(x: f64, y: f64) -> f64 {
    let mut value = x % y;
    if (value < 0.0 && y > 0.0) || (value > 0.0 && y < 0.0) {
        value += y;
    }
    value + 0.0
}

/// `Animation::interpolate_via_rest`: `(from * Quaternion().slerp(rest.inverse() * to, w)).normalized()`.
fn via_rest(from: Quat, to: Quat, weight: f32, rest: Quat) -> Quat {
    let inv = [-rest[0], -rest[1], -rest[2], rest[3]];
    let step = quat_slerp([0.0, 0.0, 0.0, 1.0], quat_mul(inv, to), weight);
    quat_normalized(quat_mul(from, step))
}

/// How a track reads between keys.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Interp {
    Nearest,
    Linear,
}

/// One position or rotation track of one bone: key times and their values (3 or 4 floats a key).
#[derive(Clone, Debug)]
pub struct Track {
    pub bone: usize,
    pub rotation: bool,
    pub interp: Interp,
    pub loop_wrap: bool,
    pub times: Vec<f64>,
    pub values: Vec<f32>,
}

/// One clip: its length, whether it loops (`LOOP_LINEAR`), its tracks in the clip's order and the
/// distinct bones they touch.
#[derive(Clone, Debug)]
pub struct Clip {
    pub name: String,
    pub length: f64,
    pub looped: bool,
    pub tracks: Vec<Track>,
    pub bones: Vec<usize>,
}

impl Clip {
    /// A clip of `tracks`, its bone list derived.
    pub fn new(name: String, length: f64, looped: bool, tracks: Vec<Track>) -> Self {
        let mut bones: Vec<usize> = Vec::new();
        for t in &tracks {
            if !bones.contains(&t.bone) {
                bones.push(t.bone);
            }
        }
        Self {
            name,
            length,
            looped,
            tracks,
            bones,
        }
    }

    /// `Animation::_find` forward: the last key at or before `time`, -1 before the first.
    fn find(times: &[f64], time: f64) -> isize {
        let len = times.len() as isize;
        if len == 0 {
            return -2;
        }
        let (mut low, mut high, mut middle) = (0isize, len - 1, 0isize);
        while low <= high {
            middle = (low + high) / 2;
            let t = times[middle as usize];
            if eq_approx(time, t) {
                return middle;
            } else if time < t {
                high = middle - 1;
            } else {
                low = middle + 1;
            }
        }
        if times[middle as usize] > time {
            middle -= 1;
        }
        middle
    }

    /// `Animation::_interpolate` forward: the key pair and weight `track` reads at `time`, or the
    /// lone key; `None` when the track has no key inside the clip.
    fn locate(&self, track: &Track, time: f64) -> Option<(usize, usize, f32)> {
        let times = &track.times;
        let len = Self::find(times, self.length) + 1;
        if len <= 0 {
            return None;
        }
        if len == 1 {
            return Some((0, 0, 0.0));
        }
        let mut idx = Self::find(times, time);
        if idx == -2 {
            return None;
        }
        let maxi = len - 1;
        let start_edge = idx == -1;
        let end_edge = idx >= maxi;
        let next;
        let (mut delta, mut from) = (0.0f32, 0.0f32);
        if !track.loop_wrap || !self.looped {
            if start_edge {
                idx = 0;
            }
            next = (idx + 1).clamp(0, maxi);
        } else {
            if start_edge {
                idx = maxi;
            }
            next = (idx + 1).rem_euclid(len);
            let (ti, tn) = (times[idx as usize], times[next as usize]);
            if start_edge {
                let mut endtime = (self.length - ti) as f32;
                if endtime < 0.0 {
                    endtime = 0.0;
                }
                delta = (f64::from(endtime) + tn) as f32;
                from = (f64::from(endtime) + time) as f32;
            } else if end_edge {
                delta = ((self.length - ti) + tn) as f32;
                from = (time - ti) as f32;
            }
        }
        if !start_edge && !end_edge {
            delta = (times[next as usize] - times[idx as usize]) as f32;
            from = (time - times[idx as usize]) as f32;
        }
        let c = if zero_approx_f32(delta) {
            0.0
        } else {
            from / delta
        };
        Some((idx as usize, next as usize, c))
    }

    fn position(&self, track: &Track, time: f64) -> Option<Vec3> {
        let (i, n, c) = self.locate(track, time)?;
        let v = &track.values;
        let a = [v[i * 3], v[i * 3 + 1], v[i * 3 + 2]];
        if i == n && c == 0.0 || track.interp == Interp::Nearest {
            return Some(a);
        }
        let b = [v[n * 3], v[n * 3 + 1], v[n * 3 + 2]];
        Some([
            lerp_f32(a[0], b[0], c),
            lerp_f32(a[1], b[1], c),
            lerp_f32(a[2], b[2], c),
        ])
    }

    fn rotation(&self, track: &Track, time: f64) -> Option<Quat> {
        let (i, n, c) = self.locate(track, time)?;
        let v = &track.values;
        let a = [v[i * 4], v[i * 4 + 1], v[i * 4 + 2], v[i * 4 + 3]];
        if i == n && c == 0.0 || track.interp == Interp::Nearest {
            return Some(a);
        }
        let b = [v[n * 4], v[n * 4 + 1], v[n * 4 + 2], v[n * 4 + 3]];
        Some(quat_slerp(a, b, c))
    }
}

/// Every clip of one library on one skeleton, with the mixer's per-bone setup: the rest each pass
/// starts from and which components any clip animates.
#[derive(Clone, Debug, Default)]
pub struct ClipSet {
    pub clips: Vec<Clip>,
    pub init_loc: Vec<Vec3>,
    pub init_rot: Vec<Quat>,
    pub loc_used: Vec<bool>,
    pub rot_used: Vec<bool>,
    pub motion_scale: f32,
}

impl ClipSet {
    /// A set over `bone_count` bones with these rests; [`ClipSet::add`] the clips after.
    pub fn new(init_loc: Vec<Vec3>, init_rot: Vec<Quat>, motion_scale: f32) -> Self {
        let n = init_loc.len();
        Self {
            clips: Vec::new(),
            init_loc,
            init_rot,
            loc_used: vec![false; n],
            rot_used: vec![false; n],
            motion_scale,
        }
    }

    pub fn bone_count(&self) -> usize {
        self.init_loc.len()
    }

    /// Adds `clip`, marking the components it animates; its index.
    pub fn add(&mut self, clip: Clip) -> usize {
        for t in &clip.tracks {
            if t.rotation {
                self.rot_used[t.bone] = true;
            } else {
                self.loc_used[t.bone] = true;
            }
        }
        self.clips.push(clip);
        self.clips.len() - 1
    }

    pub fn find(&self, name: &str) -> Option<usize> {
        self.clips.iter().position(|c| c.name == name)
    }
}

/// `AnimationPlayer::PlaybackData`: a clip and where it is.
#[derive(Clone, Copy, Debug)]
struct Data {
    clip: usize,
    pos: f64,
}

/// `AnimationPlayer::Blend`: a clip fading out.
#[derive(Clone, Copy, Debug)]
struct Blend {
    data: Data,
    left: f32,
    time: f64,
}

/// One figure's playback, the poses of its last call waiting in [`Player::written`].
#[derive(Clone, Debug)]
pub struct Player {
    current: Option<Data>,
    assigned: Option<usize>,
    blends: Vec<Blend>,
    started: bool,
    playing: bool,
    end_reached: bool,
    pub speed_scale: f32,
    /// Bones left alone (a far figure's fingers): never sampled nor written.
    skip: Vec<bool>,
    instances: Vec<(usize, f64, f32)>,
    loc: Vec<Vec3>,
    rot: Vec<Quat>,
    total: Vec<f32>,
    /// Per bone: bit 1 position, bit 2 rotation, written since [`Player::clear_written`].
    pub written: Vec<u8>,
    pub out_loc: Vec<Vec3>,
    pub out_rot: Vec<Quat>,
}

impl Player {
    pub fn new(bone_count: usize) -> Self {
        Self {
            current: None,
            assigned: None,
            blends: Vec::new(),
            started: false,
            playing: false,
            end_reached: false,
            speed_scale: 1.0,
            skip: vec![false; bone_count],
            instances: Vec::new(),
            loc: vec![[0.0; 3]; bone_count],
            rot: vec![[0.0, 0.0, 0.0, 1.0]; bone_count],
            total: vec![0.0; bone_count],
            written: vec![0; bone_count],
            out_loc: vec![[0.0; 3]; bone_count],
            out_rot: vec![[0.0, 0.0, 0.0, 1.0]; bone_count],
        }
    }

    /// Leaves the bones `skip` marks alone from now on (none when empty).
    pub fn set_skip(&mut self, skip: &[bool]) {
        for (i, s) in self.skip.iter_mut().enumerate() {
            *s = skip.get(i).copied().unwrap_or(false);
        }
    }

    pub fn clear_written(&mut self) {
        self.written.fill(0);
    }

    /// `current_animation`: the assigned clip while playing.
    pub fn current_animation(&self) -> Option<usize> {
        if self.playing { self.assigned } else { None }
    }

    pub fn current_position(&self, set: &ClipSet) -> f64 {
        let _ = set;
        self.current.map_or(0.0, |c| c.pos)
    }

    pub fn current_length(&self, set: &ClipSet) -> f64 {
        self.current.map_or(0.0, |c| set.clips[c.clip].length)
    }

    pub fn is_playing(&self) -> bool {
        self.playing
    }

    fn current_blend_amount(&self) -> f32 {
        let mut blend = 1.0f32;
        for b in &self.blends {
            blend -= b.left;
        }
        blend.max(0.0)
    }

    /// `AnimationPlayer::play(name, custom_blend)`.
    pub fn play(&mut self, set: &ClipSet, clip: usize, custom_blend: f64) {
        if let Some(cur) = self.current {
            let blend_time = if greater_or_eq_approx(custom_blend, 0.0) {
                custom_blend
            } else {
                0.0
            };
            if greater_approx(blend_time, 0.0) {
                let left = self.current_blend_amount();
                self.blends.push(Blend {
                    data: cur,
                    left,
                    time: blend_time,
                });
            } else {
                self.blends.clear();
            }
        }
        let pos = self.current.map_or(0.0, |c| c.pos);
        self.current = Some(Data { clip, pos });
        if self.assigned != Some(clip) {
            self.current = Some(Data { clip, pos: 0.0 });
            self.assigned = Some(clip);
        } else if greater_or_eq_approx(pos, set.clips[clip].length) {
            self.seek_internal(set, 0.0, true);
        } else if self.playing {
            return;
        }
        self.started = true;
        self.playing = true;
    }

    /// `AnimationPlayer::seek(time, update)`.
    pub fn seek(&mut self, set: &ClipSet, time: f64, update: bool) {
        self.seek_internal(set, time, update);
    }

    fn seek_internal(&mut self, set: &ClipSet, time: f64, update: bool) {
        self.after_start(set);
        match (self.current.as_mut(), self.assigned) {
            (Some(c), _) => c.pos = time,
            (None, Some(a)) => self.current = Some(Data { clip: a, pos: time }),
            (None, None) => return,
        }
        if update {
            self.process(set, 0.0);
        }
    }

    /// `AnimationPlayer::advance(delta)`.
    pub fn advance(&mut self, set: &ClipSet, delta: f64) {
        self.after_start(set);
        self.process(set, delta);
    }

    /// `_check_immediately_after_start`.
    fn after_start(&mut self, set: &ClipSet) {
        if self.started {
            self.process(set, 0.0);
        }
    }

    /// `AnimationMixer::_process_animation`.
    fn process(&mut self, set: &ClipSet, delta: f64) {
        let Some(mut cur) = self.current else {
            return;
        };
        self.end_reached = false;
        let started = self.started;
        self.started = false;
        self.instances.clear();
        let amount = self.current_blend_amount();
        self.playback(set, &mut cur, delta, amount, started, true);
        self.current = Some(cur);
        if self.end_reached {
            self.blends.clear();
        } else {
            let mut i = 0;
            while i < self.blends.len() {
                let mut b = self.blends[i];
                let left = (f64::from(b.left)
                    - (f64::from(self.speed_scale) * delta).abs() / b.time)
                    .max(0.0);
                b.left = left as f32;
                let erase = less_or_eq_approx(f64::from(b.left), 0.0);
                if erase {
                    b.left = EPS as f32;
                }
                let mut data = b.data;
                self.playback(set, &mut data, delta, b.left, false, false);
                b.data = data;
                self.blends[i] = b;
                if erase {
                    self.blends.remove(i);
                } else {
                    i += 1;
                }
            }
        }
        self.blend(set);
        if self.end_reached {
            self.playing = false;
        }
    }

    /// `_process_playback_data`: moves `cd` on and queues its instance.
    fn playback(
        &mut self,
        set: &ClipSet,
        cd: &mut Data,
        p_delta: f64,
        mut weight: f32,
        started: bool,
        current: bool,
    ) {
        let clip = &set.clips[cd.clip];
        let speed = f64::from(self.speed_scale);
        let backwards = speed.is_sign_negative();
        let delta = if started { 0.0 } else { p_delta * speed };
        let mut next = cd.pos + delta;
        let (start, end) = (0.0, clip.length);
        if clip.looped {
            next = fposmod(next - start, end - start) + start;
        } else if less_approx(next, start) {
            next = start;
        } else if greater_approx(next, end) {
            next = end;
        }
        let prev = cd.pos;
        if current && !clip.looped {
            if !backwards && less_or_eq_approx(prev, end) && eq_approx(next, end) {
                next = end;
                self.end_reached = true;
                weight = 1.0;
            }
            if backwards && greater_or_eq_approx(prev, start) && eq_approx(next, start) {
                next = start;
                self.end_reached = true;
                weight = 1.0;
            }
        }
        cd.pos = next;
        self.instances
            .push((cd.clip, if started { prev } else { next }, weight));
    }

    /// The mixer's pass over the queued instances, its result into the written poses.
    fn blend(&mut self, set: &ClipSet) {
        let n = set.bone_count();
        self.loc.copy_from_slice(&set.init_loc[..n]);
        self.rot.copy_from_slice(&set.init_rot[..n]);
        self.total.fill(0.0);
        for &(clip, _, weight) in &self.instances {
            if zero_approx_f32(weight) {
                continue;
            }
            for &b in &set.clips[clip].bones {
                self.total[b] += weight;
            }
        }
        let ms = set.motion_scale;
        for &(ci, time, weight) in &self.instances {
            let clip = &set.clips[ci];
            for t in &clip.tracks {
                let b = t.bone;
                if self.skip[b] || zero_approx_f32(self.total[b]) {
                    continue;
                }
                let blend = weight / self.total[b];
                if zero_approx_f32(blend) {
                    continue;
                }
                if t.rotation {
                    if let Some(q) = clip.rotation(t, time) {
                        self.rot[b] = via_rest(self.rot[b], q, blend, set.init_rot[b]);
                    }
                } else if let Some(p) = clip.position(t, time) {
                    let init = set.init_loc[b];
                    for k in 0..3 {
                        self.loc[b][k] += (p[k] * ms - init[k]) * blend;
                    }
                }
            }
        }
        let touched = |b: usize| set.loc_used[b] || set.rot_used[b];
        for b in 0..n {
            if self.skip[b] || !touched(b) || zero_approx_f32(self.total[b]) {
                continue;
            }
            if set.loc_used[b] {
                self.out_loc[b] = self.loc[b];
                self.written[b] |= 1;
            }
            if set.rot_used[b] {
                self.out_rot[b] = self.rot[b];
                self.written[b] |= 2;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rot_track(bone: usize, times: &[f64], quats: &[Quat]) -> Track {
        Track {
            bone,
            rotation: true,
            interp: Interp::Linear,
            loop_wrap: true,
            times: times.to_vec(),
            values: quats.iter().flatten().copied().collect(),
        }
    }

    fn pos_track(bone: usize, times: &[f64], ps: &[Vec3]) -> Track {
        Track {
            bone,
            rotation: false,
            interp: Interp::Linear,
            loop_wrap: true,
            times: times.to_vec(),
            values: ps.iter().flatten().copied().collect(),
        }
    }

    fn yaw(a: f32) -> Quat {
        [0.0, (a * 0.5).sin(), 0.0, (a * 0.5).cos()]
    }

    fn set() -> ClipSet {
        let mut s = ClipSet::new(
            vec![[0.0, 1.0, 0.0], [0.0, 0.5, 0.0]],
            vec![[0.0, 0.0, 0.0, 1.0]; 2],
            1.0,
        );
        s.add(Clip::new(
            "walk".into(),
            1.0,
            true,
            vec![
                pos_track(
                    0,
                    &[0.0, 0.5, 1.0],
                    &[[0.0, 1.0, 0.0], [0.0, 1.2, 0.0], [0.0, 1.0, 0.0]],
                ),
                rot_track(1, &[0.0, 0.5], &[yaw(0.0), yaw(1.0)]),
            ],
        ));
        s.add(Clip::new(
            "land".into(),
            0.4,
            false,
            vec![rot_track(1, &[0.0, 0.4], &[yaw(-1.0), yaw(0.5)])],
        ));
        s
    }

    #[test]
    fn find_matches_the_engine() {
        let t = [0.0, 0.5, 1.0];
        assert_eq!(Clip::find(&t, -0.1), -1);
        assert_eq!(Clip::find(&t, 0.0), 0);
        assert_eq!(Clip::find(&t, 0.25), 0);
        assert_eq!(Clip::find(&t, 0.5), 1);
        assert_eq!(Clip::find(&t, 0.500001), 1);
        assert_eq!(Clip::find(&t, 2.0), 2);
    }

    #[test]
    fn looped_track_wraps_past_its_last_key() {
        let s = set();
        let clip = &s.clips[0];
        // Rotation keys at 0 and 0.5 in a 1 s loop: past 0.5 it turns back towards the first key.
        let (i, n, c) = clip.locate(&clip.tracks[1], 0.75).unwrap();
        assert_eq!((i, n), (1, 0));
        assert!((c - 0.5).abs() < 1e-6);
        let p = clip.position(&clip.tracks[0], 0.25).unwrap();
        assert!((p[1] - 1.1).abs() < 1e-6);
    }

    #[test]
    fn one_shot_clip_ends_and_stops_playing() {
        let s = set();
        let mut p = Player::new(2);
        p.play(&s, 1, 0.0);
        p.advance(&s, 0.3);
        assert_eq!(p.current_animation(), Some(1));
        p.advance(&s, 0.3);
        assert_eq!(p.current_animation(), None);
        assert!((p.current_position(&s) - 0.4).abs() < 1e-12);
        let q = p.out_rot[1];
        let want = yaw(0.5);
        for k in 0..4 {
            assert!((q[k] - want[k]).abs() < 1e-6);
        }
    }

    #[test]
    fn cross_fade_moves_from_old_to_new_clip() {
        let s = set();
        let mut p = Player::new(2);
        p.play(&s, 0, 0.0);
        p.advance(&s, 0.1);
        p.play(&s, 1, 0.2);
        p.advance(&s, 0.1);
        // Half way through the fade both clips weigh in: the rotation lies between the two.
        let half = p.out_rot[1];
        p.advance(&s, 0.15);
        assert_eq!(p.blends.len(), 0);
        assert!(half[1].abs() > 0.0);
        assert_eq!(p.written[0] & 1, 1);
    }

    #[test]
    fn skipped_bones_are_left_alone() {
        let s = set();
        let mut p = Player::new(2);
        p.set_skip(&[false, true]);
        p.play(&s, 0, 0.0);
        p.advance(&s, 0.1);
        assert_eq!(p.written, vec![1, 0]);
    }
}
