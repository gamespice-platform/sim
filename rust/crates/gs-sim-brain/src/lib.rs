//! The hot half of a gs-bot brain's think: [`aim`] (GSBotAim: view, aim error, reaction,
//! trigger), [`fight`] (GSBotBrain's fight step: range, strafing, crouch, jump, the legs) and
//! [`act`] (its roaming steer: the walking or standing look, the glance, the turn, the legs).
//!
//! Each is the script's arithmetic in the script's order and precision (engine vectors `f32`,
//! script floats `f64`, the script's random stream), so a brain thinking natively makes the same
//! decisions, to the bit, as the GDScript it replaces; the addon's parity suite holds it to that.

pub mod act;
pub mod aim;
pub mod fight;
pub mod hear;
pub mod num;
pub mod pickup;
pub mod tricks;

pub use aim::{Aim, AimProfile, View};
pub use fight::{FightIn, FightOut, FightProfile, FirstShot, Strafe, fight};
pub use num::{Stream, Vec2, Vec3};
