//! One hitscan pellet through the world and the hitbox sets: gs-combat's `GSPelletTracer` walk
//! (walls, sheets, ricochets and bodies spend penetration power), `GSTrace.ray` (world first,
//! then the nearest set, ties to the wall, the reverse wall recheck) and `GSHitbox` shapes.
//!
//! The world is a [`WorldTrace`]: the engine's physics ray today, a collision model later.
//! Vectors are the engine's f32, scalars the script's f64, in the script's order. What the
//! walk found comes back as fixed-stride [`Event`] rows the caller turns into damage.

/// An engine `Vector3`.
pub type V3 = [f32; 3];

/// An engine `Transform3D`: basis rows and origin.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Xform {
    pub b: [[f32; 3]; 3],
    pub o: V3,
}

impl Xform {
    pub const IDENTITY: Xform = Xform {
        b: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
        o: [0.0; 3],
    };

    /// `basis * v`.
    pub fn basis_xform(&self, v: V3) -> V3 {
        [dot(self.b[0], v), dot(self.b[1], v), dot(self.b[2], v)]
    }

    /// `transform * point`.
    pub fn xform(&self, v: V3) -> V3 {
        add(self.basis_xform(v), self.o)
    }

    /// `Transform3D.affine_inverse()`, the engine's cofactor inverse.
    pub fn affine_inverse(&self) -> Xform {
        let r = &self.b;
        let co0 = r[1][1] * r[2][2] - r[1][2] * r[2][1];
        let co1 = r[1][2] * r[2][0] - r[1][0] * r[2][2];
        let co2 = r[1][0] * r[2][1] - r[1][1] * r[2][0];
        let det = r[0][0] * co0 + r[0][1] * co1 + r[0][2] * co2;
        let s = 1.0 / det;
        let b = [
            [
                co0 * s,
                (r[0][2] * r[2][1] - r[0][1] * r[2][2]) * s,
                (r[0][1] * r[1][2] - r[0][2] * r[1][1]) * s,
            ],
            [
                co1 * s,
                (r[0][0] * r[2][2] - r[0][2] * r[2][0]) * s,
                (r[0][2] * r[1][0] - r[0][0] * r[1][2]) * s,
            ],
            [
                co2 * s,
                (r[0][1] * r[2][0] - r[0][0] * r[2][1]) * s,
                (r[0][0] * r[1][1] - r[0][1] * r[1][0]) * s,
            ],
        ];
        let inv = Xform { b, o: [0.0; 3] };
        Xform {
            b,
            o: inv.basis_xform(neg(self.o)),
        }
    }

    /// `self * other`.
    pub fn mul(&self, other: &Xform) -> Xform {
        let a = &self.b;
        let m = &other.b;
        let mut b = [[0.0f32; 3]; 3];
        for (i, row) in b.iter_mut().enumerate() {
            for (j, cell) in row.iter_mut().enumerate() {
                *cell = m[0][j] * a[i][0] + m[1][j] * a[i][1] + m[2][j] * a[i][2];
            }
        }
        Xform {
            b,
            o: self.xform(other.o),
        }
    }
}

pub fn add(a: V3, b: V3) -> V3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

pub fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

pub fn neg(a: V3) -> V3 {
    [-a[0], -a[1], -a[2]]
}

/// `Vector3 * float`: the script's float narrowed to the engine's real first.
pub fn scale(a: V3, s: f64) -> V3 {
    let s = s as f32;
    [a[0] * s, a[1] * s, a[2] * s]
}

pub fn dot(a: V3, b: V3) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

pub fn length(a: V3) -> f32 {
    dot(a, a).sqrt()
}

pub fn distance(a: V3, b: V3) -> f32 {
    length(sub(b, a))
}

pub fn normalized(a: V3) -> V3 {
    let l2 = dot(a, a);
    if l2 == 0.0 {
        return [0.0; 3];
    }
    let l = l2.sqrt();
    [a[0] / l, a[1] / l, a[2] / l]
}

/// `Vector3.bounce(n)`: minus `2 * n * dot(n) - self`.
pub fn bounce(a: V3, n: V3) -> V3 {
    let d = dot(a, n);
    let r = sub(scale_f32(scale_f32(n, 2.0), d), a);
    neg(r)
}

fn scale_f32(a: V3, s: f32) -> V3 {
    [a[0] * s, a[1] * s, a[2] * s]
}

/// `GSHitbox.Shape`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shape {
    Capsule = 0,
    Box = 1,
    Sphere = 2,
}

impl Shape {
    pub fn from_code(code: i64) -> Shape {
        match code {
            1 => Shape::Box,
            2 => Shape::Sphere,
            _ => Shape::Capsule,
        }
    }
}

/// One hitbox where the ray meets it: its world transform (and inverse) and its size.
#[derive(Clone, Copy, Debug)]
pub struct Hitbox {
    pub global: Xform,
    pub inverse: Xform,
    pub shape: Shape,
    pub radius: f64,
    pub height: f64,
    pub half: V3,
    pub damage_scale: f64,
    pub precedence: i64,
    pub enabled: bool,
}

impl Hitbox {
    pub fn new(global: Xform, shape: Shape, radius: f64, height: f64, half: V3) -> Hitbox {
        Hitbox {
            global,
            inverse: global.affine_inverse(),
            shape,
            radius,
            height,
            half,
            damage_scale: 1.0,
            precedence: 0,
            enabled: true,
        }
    }

    /// `GSHitbox.intersect_ray`: world distance, point and unit normal, or None.
    pub fn intersect(&self, from: V3, dir: V3, max: f64) -> Option<(f64, V3, V3)> {
        if !self.enabled {
            return None;
        }
        let local_from = self.inverse.xform(from);
        let local_dir = normalized(self.inverse.basis_xform(dir));
        let d = match self.shape {
            Shape::Sphere => ray_sphere(local_from, local_dir, [0.0; 3], self.radius),
            Shape::Box => ray_box(local_from, local_dir, self.half),
            Shape::Capsule => self.ray_capsule(local_from, local_dir),
        };
        if d < 0.0 {
            return None;
        }
        let local_hit = add(local_from, scale(local_dir, d));
        let world_hit = self.global.xform(local_hit);
        let world_distance = distance(from, world_hit) as f64;
        if world_distance > max {
            return None;
        }
        Some((
            world_distance,
            world_hit,
            normalized(self.normal_at(local_hit)),
        ))
    }

    fn half_axis(&self) -> f64 {
        (self.height * 0.5 - self.radius).max(0.0)
    }

    fn ray_capsule(&self, from: V3, dir: V3) -> f64 {
        let half = self.half_axis();
        let dx = dir[0] as f64;
        let dz = dir[2] as f64;
        let a = dx * dx + dz * dz;
        let mut best = -1.0f64;
        if a > 0.000001 {
            let ox = from[0] as f64;
            let oz = from[2] as f64;
            let b = ox * dx + oz * dz;
            let c = ox * ox + oz * oz - self.radius * self.radius;
            let disc = b * b - a * c;
            if disc >= 0.0 {
                let root = disc.sqrt();
                for t in [(-b - root) / a, (-b + root) / a] {
                    if t < 0.0 {
                        continue;
                    }
                    let y = from[1] as f64 + dir[1] as f64 * t;
                    if y >= -half && y <= half {
                        best = if best < 0.0 { t } else { best.min(t) };
                        break;
                    }
                }
            }
        }
        for cap_y in [-half, half] {
            let t = ray_sphere(from, dir, [0.0, cap_y as f32, 0.0], self.radius);
            if t >= 0.0 {
                best = if best < 0.0 { t } else { best.min(t) };
            }
        }
        if best < 0.0 {
            let on_axis = [0.0, (from[1] as f64).clamp(-half, half) as f32, 0.0];
            if distance(from, on_axis) as f64 <= self.radius {
                return 0.0;
            }
        }
        best
    }

    fn normal_at(&self, p: V3) -> V3 {
        match self.shape {
            Shape::Sphere => self.global.basis_xform(p),
            Shape::Box => {
                let s = [
                    (p[0] as f64 / (self.half[0] as f64).max(0.0001)) as f32,
                    (p[1] as f64 / (self.half[1] as f64).max(0.0001)) as f32,
                    (p[2] as f64 / (self.half[2] as f64).max(0.0001)) as f32,
                ];
                let mut axis = 0;
                for i in 1..3 {
                    if s[i].abs() > s[axis].abs() {
                        axis = i;
                    }
                }
                let mut n = [0.0f32; 3];
                n[axis] = sign(s[axis]);
                self.global.basis_xform(n)
            }
            Shape::Capsule => {
                let half = self.half_axis();
                let on_axis = [0.0, (p[1] as f64).clamp(-half, half) as f32, 0.0];
                self.global.basis_xform(sub(p, on_axis))
            }
        }
    }
}

fn sign(v: f32) -> f32 {
    if v > 0.0 {
        1.0
    } else if v < 0.0 {
        -1.0
    } else {
        0.0
    }
}

/// `GSHitbox._ray_sphere`: starting inside is a hit at 0.
pub fn ray_sphere(from: V3, dir: V3, centre: V3, r: f64) -> f64 {
    let offset = sub(from, centre);
    let b = dot(offset, dir) as f64;
    let c = dot(offset, offset) as f64 - r * r;
    if c <= 0.0 {
        return 0.0;
    }
    let disc = b * b - c;
    if disc < 0.0 {
        return -1.0;
    }
    let t = -b - disc.sqrt();
    if t >= 0.0 { t } else { -1.0 }
}

/// `GSHitbox._ray_box`: slabs, starting inside is 0.
pub fn ray_box(from: V3, dir: V3, ext: V3) -> f64 {
    let mut t_min = f64::NEG_INFINITY;
    let mut t_max = f64::INFINITY;
    for axis in 0..3 {
        let d = dir[axis] as f64;
        let o = from[axis] as f64;
        let e = ext[axis] as f64;
        if d.abs() < 0.000001 {
            if o < -e || o > e {
                return -1.0;
            }
            continue;
        }
        let mut t1 = (-e - o) / d;
        let mut t2 = (e - o) / d;
        if t1 > t2 {
            std::mem::swap(&mut t1, &mut t2);
        }
        t_min = t_min.max(t1);
        t_max = t_max.min(t2);
        if t_min > t_max {
            return -1.0;
        }
    }
    if t_max < 0.0 {
        return -1.0;
    }
    t_min.max(0.0)
}

/// One entity's hitboxes: a `GSHitboxSet` with its bounds sphere.
#[derive(Clone, Debug, Default)]
pub struct HitSet {
    pub entity: i64,
    pub enabled: bool,
    pub centre: V3,
    pub radius: f64,
    pub boxes: Vec<Hitbox>,
}

/// `GSHitboxSet._ray_hits_bounds` (and `GSTrace.ray`'s inline copy of it).
fn hits_bounds(set: &HitSet, from: V3, dir: V3, limit: f64) -> bool {
    let offset = sub(from, set.centre);
    let gap = dot(offset, offset) as f64 - set.radius * set.radius;
    let along = dot(offset, dir) as f64;
    if gap <= 0.0 {
        return true;
    }
    if along > 0.0 {
        return false;
    }
    let depth = along * along - gap;
    depth >= 0.0 && -along - depth.sqrt() <= limit
}

/// A world surface the ray met, with what the walk needs to know of its material.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WorldHit {
    pub distance: f64,
    pub point: V3,
    pub normal: V3,
    /// The caller's index for the surface material, handed back in events.
    pub material: i64,
    /// `GSPenetration.resistance_of(material)`.
    pub resistance: f64,
    /// Whether the material is one of the round's ricochet materials.
    pub ricochet: bool,
    /// `GSCombatManager.sheet_thickness` of the material, 0 when unknown.
    pub sheet: f64,
    /// A breakable pane: the walk stops with [`Event::Script`], the script path owns panes.
    pub pane: bool,
}

/// The world ray seam: physics today, a collision model later.
pub trait WorldTrace {
    fn ray(&mut self, from: V3, dir: V3, max: f64) -> Option<WorldHit>;
}

/// What a ray found: the world, or set `set`'s box `index`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Found {
    World {
        hit: WorldHit,
        inside_start: bool,
    },
    Body {
        set: usize,
        index: usize,
        distance: f64,
        point: V3,
        normal: V3,
    },
}

impl Found {
    pub fn distance(&self) -> f64 {
        match self {
            Found::World { hit, .. } => hit.distance,
            Found::Body { distance, .. } => *distance,
        }
    }
}

/// `GSTrace.WALL_RECHECK_MIN`.
pub const WALL_RECHECK_MIN: f64 = 0.02;

/// `GSHitboxSet.intersect_ray`: the nearest box, a tie within 0.1 mm to precedence, then damage scale.
fn set_ray(set: &HitSet, from: V3, dir: V3, limit: f64) -> Option<(usize, f64, V3, V3)> {
    if !set.enabled || set.boxes.is_empty() || !hits_bounds(set, from, dir, limit) {
        return None;
    }
    let mut best: Option<(usize, f64, V3, V3)> = None;
    for (i, b) in set.boxes.iter().enumerate() {
        let Some((d, p, n)) = b.intersect(from, dir, limit) else {
            continue;
        };
        let Some((bi, bd, _, _)) = best else {
            best = Some((i, d, p, n));
            continue;
        };
        if d < bd - 0.0001 {
            best = Some((i, d, p, n));
            continue;
        }
        if (d - bd).abs() <= 0.0001 {
            let inc = &set.boxes[bi];
            if b.precedence > inc.precedence
                || (b.precedence == inc.precedence && b.damage_scale > inc.damage_scale)
            {
                best = Some((i, d, p, n));
            }
        }
    }
    best
}

/// `GSTrace.ray`: the world, then every set not in `skip` nearer than it; a body hit is rechecked
/// back toward the muzzle for a wall the forward ray started inside.
pub fn trace_ray(
    world: &mut dyn WorldTrace,
    sets: &[HitSet],
    skip: &[bool],
    from: V3,
    dir: V3,
    max: f64,
) -> Option<Found> {
    let w = world.ray(from, dir, max);
    let mut limit = w.map_or(max, |h| h.distance);
    let mut best = w.map(|hit| Found::World {
        hit,
        inside_start: false,
    });
    for (i, set) in sets.iter().enumerate() {
        if skip.get(i).copied().unwrap_or(false) {
            continue;
        }
        let Some((index, d, p, n)) = set_ray(set, from, dir, limit) else {
            continue;
        };
        if d > limit {
            continue;
        }
        if let Some(b) = best
            && d >= b.distance()
        {
            continue;
        }
        best = Some(Found::Body {
            set: i,
            index,
            distance: d,
            point: p,
            normal: n,
        });
        limit = d;
    }
    if let Some(Found::Body {
        distance: bd,
        point,
        ..
    }) = best
        && bd > WALL_RECHECK_MIN
        && let Some(mut back) = world.ray(point, neg(dir), bd - WALL_RECHECK_MIN)
    {
        back.distance = bd - back.distance;
        best = Some(Found::World {
            hit: back,
            inside_start: true,
        });
    }
    best
}

/// A round's `GSPenetration` (already scaled by the manager) and the manager's switches.
#[derive(Clone, Debug, Default)]
pub struct Pen {
    pub power: f64,
    pub max_depth: f64,
    pub body_cost: f64,
    pub speed: f64,
    pub reference_speed: f64,
    pub min_damage: f64,
    pub max_passes: i64,
    pub ricochet_angle: f64,
    pub ricochet_keep: f64,
    pub max_ricochets: i64,
    /// `GSCombatManager.ricochet_enabled`.
    pub ricochet_enabled: bool,
    /// `GSCombatManager.penetration_players`.
    pub players: bool,
    /// Body cost multiplier per hit-group index (`body_zones`, coarse or not as the rules say).
    pub zones: Vec<f64>,
}

impl Pen {
    pub fn speed_at(&self, left: f64) -> f64 {
        if self.speed <= 0.0 || self.power <= 0.0 {
            return 0.0;
        }
        self.speed * (left / self.power).clamp(0.0, 1.0).sqrt()
    }

    pub fn speed_factor(&self, left: f64) -> f64 {
        if self.speed <= 0.0 || self.reference_speed <= 0.0 {
            return 1.0;
        }
        let now = self.speed_at(left);
        if now <= 0.0 {
            return 4.0;
        }
        (self.reference_speed / now).clamp(0.25, 4.0)
    }

    pub fn strength(&self, left: f64) -> f64 {
        if self.power <= 0.0 || left <= 0.0 {
            return 0.0;
        }
        (left / self.power).clamp(0.0, 1.0).max(self.min_damage)
    }

    fn zone(&self, group: usize) -> f64 {
        self.zones.get(group).copied().unwrap_or(1.0)
    }
}

/// A body hit's per-box facts the walk needs beyond the shape.
#[derive(Clone, Copy, Debug, Default)]
pub struct BoxInfo {
    /// Index into [`Pen::zones`].
    pub group: usize,
}

/// What a pellet did, in walk order.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Event {
    /// Where the pellet stopped (or its full range in open air).
    Impact { point: V3 },
    /// A body hit: damage before the resolver is `damage * strength`, then the box's scale.
    Body {
        set: usize,
        index: usize,
        point: V3,
        normal: V3,
        /// Hit distance plus what the pellet walked before it.
        distance: f64,
        strength: f64,
        passes: i64,
        walls: i64,
        bounces: i64,
    },
    /// A wall or sheet crossed.
    Pass {
        material: i64,
        thickness: f64,
        cost: f64,
        entry: V3,
        exit: V3,
        entry_normal: V3,
        exit_normal: V3,
        sheet: bool,
    },
    /// A bounce.
    Ricochet {
        material: i64,
        point: V3,
        normal: V3,
        angle: f64,
        direction: V3,
    },
    /// A pane: the script path must trace this pellet.
    Script,
}

/// `GSPelletTracer.trace_pellet` for one pellet. `skip` is per set (the shooter and pass-through
/// sets), grown with every body the pellet passes. `info[set][box]` gives each box's group.
#[allow(clippy::too_many_arguments)]
pub fn walk(
    world: &mut dyn WorldTrace,
    sets: &[HitSet],
    info: &[Vec<BoxInfo>],
    skip: &mut Vec<bool>,
    pen: Option<&Pen>,
    origin: V3,
    direction: V3,
    max_range: f64,
    out: &mut Vec<Event>,
) {
    skip.resize(sets.len(), false);
    let mut dir = direction;
    let mut from = origin;
    let mut remaining = max_range;
    let mut power = pen.map_or(0.0, |p| p.power);
    let mut passes = 0i64;
    let mut walls = 0i64;
    let mut walked = 0.0f64;
    let mut bounces = 0i64;
    let advance = |to: V3,
                   d: f64,
                   from: &mut V3,
                   walked: &mut f64,
                   remaining: &mut f64,
                   out: &mut Vec<Event>|
     -> bool {
        *walked += d;
        *from = to;
        *remaining = max_range - *walked;
        if *remaining <= 0.0 {
            out.push(Event::Impact { point: *from });
            return false;
        }
        true
    };
    loop {
        let Some(found) = trace_ray(world, sets, skip, from, dir, remaining) else {
            out.push(Event::Impact {
                point: add(origin, scale(dir, max_range)),
            });
            return;
        };
        let can_pass = pen.is_some_and(|p| passes < p.max_passes) && power > 0.0;
        match found {
            Found::World { hit, inside_start } => {
                if hit.pane {
                    out.push(Event::Script);
                    return;
                }
                if let Some(p) = pen
                    && p.ricochet_enabled
                    && bounces < p.max_ricochets
                    && dot(hit.normal, hit.normal) as f64 >= 0.25
                {
                    let grazing = grazing_deg(dir, hit.normal);
                    if p.ricochet_angle > 0.0
                        && p.max_ricochets > 0
                        && grazing <= p.ricochet_angle
                        && hit.ricochet
                    {
                        dir = normalized(bounce(dir, hit.normal));
                        power *= p.ricochet_keep;
                        bounces += 1;
                        out.push(Event::Ricochet {
                            material: hit.material,
                            point: hit.point,
                            normal: hit.normal,
                            angle: grazing,
                            direction: dir,
                        });
                        walked += hit.distance;
                        from = add(hit.point, scale(hit.normal, 0.02));
                        remaining = max_range - walked;
                        if power <= 0.0 || remaining <= 0.0 {
                            out.push(Event::Impact { point: hit.point });
                            return;
                        }
                        continue;
                    }
                }
                let mut exit: Option<(V3, V3)> = None;
                let mut sheet = false;
                if can_pass && let Some(p) = pen {
                    exit = if inside_start {
                        Some((hit.point, hit.normal))
                    } else {
                        let far = add(hit.point, scale(dir, p.max_depth));
                        world
                            .ray(far, neg(dir), p.max_depth)
                            .map(|b| (b.point, b.normal))
                    };
                    if exit.is_none() && hit.sheet > 0.0 {
                        exit = Some((add(hit.point, scale(dir, hit.sheet)), neg(hit.normal)));
                        sheet = true;
                    }
                }
                let (Some((exit_point, exit_normal)), Some(p)) = (exit, pen) else {
                    out.push(Event::Impact { point: hit.point });
                    return;
                };
                let thickness = if inside_start {
                    distance(hit.point, from)
                } else {
                    distance(hit.point, exit_point)
                } as f64;
                let cost = thickness * hit.resistance.max(0.0) * p.speed_factor(power);
                power -= cost;
                if power <= 0.0 {
                    out.push(Event::Impact { point: hit.point });
                    return;
                }
                out.push(Event::Pass {
                    material: hit.material,
                    thickness,
                    cost,
                    entry: if inside_start { from } else { hit.point },
                    exit: exit_point,
                    entry_normal: hit.normal,
                    exit_normal,
                    sheet,
                });
                passes += 1;
                walls += 1;
                let d = if inside_start {
                    hit.distance
                } else {
                    hit.distance + thickness
                };
                if !advance(
                    add(exit_point, scale(dir, 0.01)),
                    d,
                    &mut from,
                    &mut walked,
                    &mut remaining,
                    out,
                ) {
                    return;
                }
            }
            Found::Body {
                set,
                index,
                distance: d,
                point,
                normal,
            } => {
                if sets[set].entity == 0 {
                    out.push(Event::Impact { point });
                    return;
                }
                let strength = match pen {
                    Some(p) if passes > 0 || bounces > 0 => p.strength(power),
                    _ => 1.0,
                };
                out.push(Event::Body {
                    set,
                    index,
                    point,
                    normal,
                    distance: d + walked,
                    strength,
                    passes,
                    walls,
                    bounces,
                });
                let Some(p) = pen.filter(|p| can_pass && p.players) else {
                    out.push(Event::Impact { point });
                    return;
                };
                let group = info
                    .get(set)
                    .and_then(|v| v.get(index))
                    .map_or(usize::MAX, |b| b.group);
                power -= p.body_cost * p.zone(group) * p.speed_factor(power);
                if power <= 0.0 {
                    out.push(Event::Impact { point });
                    return;
                }
                skip[set] = true;
                passes += 1;
                if !advance(
                    add(point, scale(dir, 0.01)),
                    d,
                    &mut from,
                    &mut walked,
                    &mut remaining,
                    out,
                ) {
                    return;
                }
            }
        }
    }
}

/// `rad_to_deg(asin(clampf(absf(dir.dot(normal)), 0, 1)))`.
pub fn grazing_deg(dir: V3, normal: V3) -> f64 {
    let s = (dot(dir, normal) as f64).abs().clamp(0.0, 1.0);
    s.asin() * (180.0 / std::f64::consts::PI)
}

/// Floats per event row: kind, then up to 19 fields (unused ones 0).
pub const EVENT_STRIDE: usize = 20;

/// Event row kinds.
pub const EV_IMPACT: f64 = 1.0;
pub const EV_BODY: f64 = 2.0;
pub const EV_PASS: f64 = 3.0;
pub const EV_RICOCHET: f64 = 4.0;
pub const EV_SCRIPT: f64 = 9.0;

impl Event {
    /// One fixed-stride row; the layout per kind is documented on the gdext class.
    pub fn write(&self, pellet: usize, out: &mut Vec<f64>) {
        let start = out.len();
        out.resize(start + EVENT_STRIDE, 0.0);
        let r = &mut out[start..];
        r[1] = pellet as f64;
        let put = |r: &mut [f64], at: usize, v: V3| {
            r[at] = v[0] as f64;
            r[at + 1] = v[1] as f64;
            r[at + 2] = v[2] as f64;
        };
        match *self {
            Event::Impact { point } => {
                r[0] = EV_IMPACT;
                put(r, 2, point);
            }
            Event::Body {
                set,
                index,
                point,
                normal,
                distance,
                strength,
                passes,
                walls,
                bounces,
            } => {
                r[0] = EV_BODY;
                put(r, 2, point);
                put(r, 5, normal);
                r[8] = set as f64;
                r[9] = index as f64;
                r[10] = distance;
                r[11] = strength;
                r[12] = passes as f64;
                r[13] = walls as f64;
                r[14] = bounces as f64;
            }
            Event::Pass {
                material,
                thickness,
                cost,
                entry,
                exit,
                entry_normal,
                exit_normal,
                sheet,
            } => {
                r[0] = EV_PASS;
                put(r, 2, entry);
                put(r, 5, exit);
                put(r, 8, entry_normal);
                put(r, 11, exit_normal);
                r[14] = material as f64;
                r[15] = thickness;
                r[16] = cost;
                r[17] = if sheet { 1.0 } else { 0.0 };
            }
            Event::Ricochet {
                material,
                point,
                normal,
                angle,
                direction,
            } => {
                r[0] = EV_RICOCHET;
                put(r, 2, point);
                put(r, 5, normal);
                put(r, 8, direction);
                r[14] = material as f64;
                r[15] = angle;
            }
            Event::Script => r[0] = EV_SCRIPT,
        }
    }
}
