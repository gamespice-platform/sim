//! The reads that do not draw: `value_at` and `value_between` (and `_jumps_after`), the
//! script's lag-compensation and bump queries, transcribed operation for operation. They count
//! nothing and never extrapolate. A blend here is the script's global `lerp` on two floats or two
//! vectors of one kind; any other pair, quaternions included, switches at the midpoint.

use super::{Interp, Other, Shared, Value, find, len2, len3, lerp32};

/// `lerp(a, b, alpha)` for a matching float or vector pair, else `a if alpha < 0.5 else b`.
fn pick<O: Other>(a: &Value<O>, b: &Value<O>, alpha: f64) -> Value<O> {
    match (a, b) {
        (Value::Float(x), Value::Float(y)) => Value::Float(x + (y - x) * alpha),
        (Value::Vec3(x), Value::Vec3(y)) => {
            let w = alpha as f32;
            Value::Vec3([
                lerp32(x[0], y[0], w),
                lerp32(x[1], y[1], w),
                lerp32(x[2], y[2], w),
            ])
        }
        (Value::Vec2(x), Value::Vec2(y)) => {
            let w = alpha as f32;
            Value::Vec2([lerp32(x[0], y[0], w), lerp32(x[1], y[1], w)])
        }
        _ if alpha < 0.5 => a.clone(),
        _ => b.clone(),
    }
}

fn get<O: Clone>(sample: &Shared<O>, key: u32) -> Option<Value<O>> {
    find(&sample.borrow().values, key).cloned()
}

/// The script's `if a == null or b == null: return b if b != null else a`, then the blend.
fn between<O: Other>(a: Option<Value<O>>, b: Option<Value<O>>, alpha: f64) -> Option<Value<O>> {
    match (a, b) {
        (Some(a), Some(b)) => Some(pick(&a, &b, alpha)),
        (a, None) => a,
        (None, b) => b,
    }
}

/// `_jumps_after`: whether `key` moves further than `cut` between two neighbouring samples from
/// index `from` on, to index `to` inclusive when given.
fn jumps_after<O: Clone>(
    samples: &[&Shared<O>],
    from: usize,
    key: u32,
    cut: f64,
    to: Option<usize>,
) -> bool {
    let end = match to {
        None => samples.len(),
        Some(t) => (t + 1).min(samples.len()),
    };
    let mut before: Option<Value<O>> = None;
    for s in samples.iter().take(end).skip(from) {
        let Some(now) = get(s, key) else {
            continue;
        };
        let far = match (&now, &before) {
            (Value::Vec3(n), Some(Value::Vec3(b))) => {
                (len3([n[0] - b[0], n[1] - b[1], n[2] - b[2]]) as f64) > cut
            }
            (Value::Vec2(n), Some(Value::Vec2(b))) => {
                (len2([n[0] - b[0], n[1] - b[1]]) as f64) > cut
            }
            _ => false,
        };
        if far {
            return true;
        }
        before = Some(now);
    }
    false
}

impl<O: Other, P> Interp<O, P> {
    /// `value_at(net_id, property, render_tick, cut)`: `None` where the script returns null.
    pub fn value_at(&self, net_id: i64, key: u32, render: f64, cut: f64) -> Option<Value<O>> {
        let track = self.tracks.get(&net_id)?;
        let samples: Vec<&Shared<O>> = track.samples.iter().collect();
        let last = samples.len().checked_sub(1)?;
        let tick = |i: usize| samples[i].borrow().tick;
        if render >= tick(last) as f64 {
            return get(samples[last], key);
        }
        if render < tick(0) as f64 {
            return None;
        }
        for i in 0..last {
            let newer = tick(i + 1) as f64;
            if render > newer {
                continue;
            }
            if cut > 0.0 && jumps_after(&samples, i, key, cut, None) {
                return None;
            }
            let older = tick(i) as f64;
            let alpha = if newer <= older {
                0.0
            } else {
                (render - older) / (newer - older)
            };
            return between(get(samples[i], key), get(samples[i + 1], key), alpha);
        }
        get(samples[last], key)
    }

    /// `value_between(net_id, property, older_tick, newer_tick, render_tick, cut)`.
    pub fn value_between(
        &self,
        net_id: i64,
        key: u32,
        older_tick: i64,
        newer_tick: i64,
        render: f64,
        cut: f64,
    ) -> Option<Value<O>> {
        let track = self.tracks.get(&net_id)?;
        let samples: Vec<&Shared<O>> = track.samples.iter().collect();
        let mut i = None;
        let mut j = None;
        for (k, s) in samples.iter().enumerate() {
            let t = s.borrow().tick;
            if t == older_tick {
                i = Some(k);
            }
            if t == newer_tick {
                j = Some(k);
            }
        }
        let (i, j) = (i?, j?);
        if cut > 0.0 && jumps_after(&samples, i, key, cut, Some(j)) {
            return None;
        }
        let alpha = if newer_tick <= older_tick {
            0.0
        } else {
            ((render - older_tick as f64) / (newer_tick - older_tick) as f64).clamp(0.0, 1.0)
        };
        between(get(samples[i], key), get(samples[j], key), alpha)
    }
}
