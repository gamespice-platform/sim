//! `GSSimClipSet` and `GSSimClipPlayer`: a figure's skeletal clips without an `AnimationPlayer`.
//!
//! The set bakes an `AnimationLibrary` on one skeleton into flat key arrays once (it is shared by
//! every figure of that body); the player is one figure's `AnimationPlayer` subset
//! (`gs_sim_clips::Player`, the engine's playback and non-deterministic blend) and writes the
//! bones its call posed straight into the skeleton. No script field is read: names, times and
//! the skeleton come in as arguments.

use std::collections::HashMap;
use std::sync::Arc;

use godot::classes::animation::{InterpolationType, LoopMode, TrackType};
use godot::classes::{AnimationLibrary, Skeleton3D};
use godot::prelude::*;
use gs_sim_clips::{Clip, ClipSet, Interp, Player, Track};
use gs_sim_math::{Vec3, rotation_quaternion};

/// One library on one skeleton, baked. Empty (and [method is_ready] false) when a clip uses
/// something the native player does not do (a scale track, an eased key, cubic keys, a compressed
/// track): the caller keeps its `AnimationPlayer` then.
#[derive(GodotClass)]
#[class(base = RefCounted, init)]
pub struct GSSimClipSet {
    set: Option<Arc<ClipSet>>,
    names: HashMap<StringName, usize>,
    list: Vec<StringName>,
    why: GString,
}

#[godot_api]
impl GSSimClipSet {
    /// Bakes every clip of [param library] whose tracks address [param skeleton] through
    /// [param skeleton_path] (the path from the player's root node, as in the tracks). True when
    /// every clip could be baked.
    #[func]
    fn bake(
        &mut self,
        library: Option<Gd<AnimationLibrary>>,
        skeleton: Option<Gd<Skeleton3D>>,
        skeleton_path: GString,
    ) -> bool {
        self.set = None;
        self.names.clear();
        self.list.clear();
        let (Some(library), Some(skeleton)) = (library, skeleton) else {
            self.why = "no library or skeleton".into();
            return false;
        };
        match bake(&library, &skeleton, &skeleton_path.to_string()) {
            Ok((set, list)) => {
                for (i, n) in list.iter().enumerate() {
                    self.names.insert(n.clone(), i);
                }
                self.list = list;
                self.set = Some(Arc::new(set));
                self.why = GString::new();
                true
            }
            Err(e) => {
                self.why = GString::from(&e);
                false
            }
        }
    }

    #[func]
    fn is_ready(&self) -> bool {
        self.set.is_some()
    }

    /// Why the last [method bake] failed; empty after a good one.
    #[func]
    fn failure(&self) -> GString {
        self.why.clone()
    }

    #[func]
    fn bone_count(&self) -> i32 {
        self.set.as_ref().map_or(0, |s| s.bone_count() as i32)
    }
}

type Baked = (ClipSet, Vec<StringName>);

fn bake(
    library: &Gd<AnimationLibrary>,
    skeleton: &Gd<Skeleton3D>,
    skeleton_path: &str,
) -> Result<Baked, String> {
    let n = skeleton.get_bone_count();
    let mut init_loc = Vec::new();
    let mut init_rot = Vec::new();
    for i in 0..n {
        let rest = skeleton.get_bone_rest(i);
        init_loc.push([rest.origin.x, rest.origin.y, rest.origin.z]);
        let r = rest.basis.rows;
        let rows = [
            Vec3::new(r[0].x, r[0].y, r[0].z),
            Vec3::new(r[1].x, r[1].y, r[1].z),
            Vec3::new(r[2].x, r[2].y, r[2].z),
        ];
        init_rot.push(rotation_quaternion(&rows));
    }
    let mut set = ClipSet::new(init_loc, init_rot, skeleton.get_motion_scale());
    let mut list = Vec::new();
    for name in library.get_animation_list().iter_shared() {
        let Some(anim) = library.get_animation(&name) else {
            continue;
        };
        let looped = match anim.get_loop_mode() {
            LoopMode::NONE => false,
            LoopMode::LINEAR => true,
            _ => return Err(format!("{name}: ping-pong loop")),
        };
        let mut tracks = Vec::new();
        for t in 0..anim.get_track_count() {
            if !anim.track_is_enabled(t) {
                continue;
            }
            let kind = anim.track_get_type(t);
            let rotation = match kind {
                TrackType::POSITION_3D => false,
                TrackType::ROTATION_3D => true,
                TrackType::SCALE_3D => return Err(format!("{name}: scale track")),
                _ => continue,
            };
            let path = anim.track_get_path(t);
            if path.get_subname_count() != 1
                || path.get_concatenated_names() != skeleton_path
            {
                continue;
            }
            let bone = skeleton.find_bone(&path.get_subname(0).to_string());
            if bone < 0 {
                continue;
            }
            if anim.track_is_compressed(t) {
                return Err(format!("{name}: compressed track"));
            }
            let interp = match anim.track_get_interpolation_type(t) {
                InterpolationType::NEAREST => Interp::Nearest,
                InterpolationType::LINEAR => Interp::Linear,
                _ => return Err(format!("{name}: cubic or angle keys")),
            };
            let keys = anim.track_get_key_count(t);
            let mut times = Vec::with_capacity(keys as usize);
            let mut values = Vec::with_capacity(keys as usize * if rotation { 4 } else { 3 });
            for k in 0..keys {
                if anim.track_get_key_transition(t, k) != 1.0 {
                    return Err(format!("{name}: eased key"));
                }
                times.push(anim.track_get_key_time(t, k));
                let v = anim.track_get_key_value(t, k);
                if rotation {
                    let q: Quaternion = v.try_to().map_err(|_| format!("{name}: rotation key"))?;
                    values.extend_from_slice(&[q.x, q.y, q.z, q.w]);
                } else {
                    let p: Vector3 = v.try_to().map_err(|_| format!("{name}: position key"))?;
                    values.extend_from_slice(&[p.x, p.y, p.z]);
                }
            }
            tracks.push(Track {
                bone: bone as usize,
                rotation,
                interp,
                loop_wrap: anim.track_get_interpolation_loop_wrap(t),
                times,
                values,
            });
        }
        set.add(Clip::new(
            name.to_string(),
            f64::from(anim.get_length()),
            looped,
            tracks,
        ));
        list.push(name);
    }
    Ok((set, list))
}

/// One figure's clip playback on one skeleton: `play`, `seek`, `advance` and the
/// `current_animation*` reads of an `AnimationPlayer`, the pose written by each call.
#[derive(GodotClass)]
#[class(base = RefCounted, init)]
pub struct GSSimClipPlayer {
    set: Option<Arc<ClipSet>>,
    names: HashMap<StringName, usize>,
    list: Vec<StringName>,
    player: Option<Player>,
    skeleton: Option<Gd<Skeleton3D>>,
}

#[godot_api]
impl GSSimClipPlayer {
    /// Plays the clips of [param set] on [param skeleton]; false when the set is not ready or not
    /// this skeleton's size.
    #[func]
    fn setup(&mut self, set: Option<Gd<GSSimClipSet>>, skeleton: Option<Gd<Skeleton3D>>) -> bool {
        let (Some(set), Some(skeleton)) = (set, skeleton) else {
            return false;
        };
        let s = set.bind();
        let Some(baked) = s.set.clone() else {
            return false;
        };
        if baked.bone_count() as i32 != skeleton.get_bone_count() {
            return false;
        }
        self.player = Some(Player::new(baked.bone_count()));
        self.names = s.names.clone();
        self.list = s.list.clone();
        self.set = Some(baked);
        self.skeleton = Some(skeleton);
        true
    }

    #[func]
    fn has_animation(&self, name: StringName) -> bool {
        self.names.contains_key(&name)
    }

    /// `AnimationPlayer.play(name, custom_blend)`; false (nothing changed) for an unknown clip.
    #[func]
    fn play(&mut self, name: StringName, custom_blend: f64) -> bool {
        let (Some(set), Some(p), Some(&clip)) = (
            self.set.as_ref(),
            self.player.as_mut(),
            self.names.get(&name),
        ) else {
            return false;
        };
        p.play(set, clip, custom_blend);
        self.flush();
        true
    }

    /// `AnimationPlayer.seek(time, update)`.
    #[func]
    fn seek(&mut self, time: f64, update: bool) {
        let (Some(set), Some(p)) = (self.set.as_ref(), self.player.as_mut()) else {
            return;
        };
        p.seek(set, time, update);
        self.flush();
    }

    /// `AnimationPlayer.advance(delta)`.
    #[func]
    fn advance(&mut self, delta: f64) {
        let (Some(set), Some(p)) = (self.set.as_ref(), self.player.as_mut()) else {
            return;
        };
        p.advance(set, delta);
        self.flush();
    }

    #[func]
    fn set_speed_scale(&mut self, scale: f32) {
        if let Some(p) = self.player.as_mut() {
            p.speed_scale = scale;
        }
    }

    #[func]
    fn get_speed_scale(&self) -> f32 {
        self.player.as_ref().map_or(1.0, |p| p.speed_scale)
    }

    /// `AnimationPlayer.current_animation`: empty when nothing plays (a one-shot clip that ended).
    #[func]
    fn current_animation(&self) -> StringName {
        self.player
            .as_ref()
            .and_then(|p| p.current_animation())
            .map_or_else(StringName::default, |i| self.list[i].clone())
    }

    #[func]
    fn current_animation_position(&self) -> f64 {
        match (self.set.as_ref(), self.player.as_ref()) {
            (Some(s), Some(p)) => p.current_position(s),
            _ => 0.0,
        }
    }

    #[func]
    fn current_animation_length(&self) -> f64 {
        match (self.set.as_ref(), self.player.as_ref()) {
            (Some(s), Some(p)) => p.current_length(s),
            _ => 0.0,
        }
    }

    /// Leaves the bones whose byte in [param skip] is not zero alone from now on (a far figure's
    /// fingers); an empty array poses every bone again.
    #[func]
    fn set_skip(&mut self, skip: PackedByteArray) {
        if let Some(p) = self.player.as_mut() {
            let mask: Vec<bool> = skip.as_slice().iter().map(|&b| b != 0).collect();
            p.set_skip(&mask);
        }
    }

    /// The bones the last calls posed, into the skeleton.
    fn flush(&mut self) {
        let (Some(p), Some(skeleton)) = (self.player.as_mut(), self.skeleton.as_mut()) else {
            return;
        };
        if !skeleton.is_instance_valid() {
            p.clear_written();
            return;
        }
        for (b, &w) in p.written.iter().enumerate() {
            if w & 1 != 0 {
                let v = p.out_loc[b];
                skeleton.set_bone_pose_position(b as i32, Vector3::new(v[0], v[1], v[2]));
            }
            if w & 2 != 0 {
                let q = p.out_rot[b];
                skeleton.set_bone_pose_rotation(b as i32, Quaternion::new(q[0], q[1], q[2], q[3]));
            }
        }
        p.clear_written();
    }
}
