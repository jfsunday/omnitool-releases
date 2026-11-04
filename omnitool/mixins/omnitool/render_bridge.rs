// Settings handed from Omnitool (horizon-client) to the camera-overlay hook
// in horizon-client-render, which cannot see the client crate. Plain data
// behind a lock, written about once per tick. Added to the render crate as
// `screen_effects::omnitool_bridge` (public); also a module of this crate.

use std::sync::Mutex;

/// What Clear Water, Low Fire and No Pumpkin Overlay want drawn differently.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Overlays {
    /// Clear Water: no underwater texture.
    pub hide_water: bool,
    /// Low Fire: no fire at all (opacity 0, fire resistance, creative).
    pub hide_fire: bool,
    /// Low Fire: the fire quad's translation in clip space, applied after the scale.
    pub fire_offset_y: f32,
    /// Low Fire: the fire quad's x/y scale.
    pub fire_scale: f32,
    /// Low Fire: factor for the fire's alpha (`opacity / 90`).
    pub fire_alpha: f32,
    /// No Pumpkin Overlay: the helmet texture (`misc/pumpkinblur`).
    pub hide_pumpkin: bool,
    /// No Pumpkin Overlay: the powder snow outline.
    pub hide_powder_snow: bool,
    /// No Pumpkin Overlay: the nether portal sprite.
    pub hide_portal: bool,
}

impl Overlays {
    pub const NONE: Overlays = Overlays {
        hide_water: false,
        hide_fire: false,
        fire_offset_y: 0.0,
        fire_scale: 1.0,
        fire_alpha: 1.0,
        hide_pumpkin: false,
        hide_powder_snow: false,
        hide_portal: false,
    };

    /// Whether any of it differs from Vanilla.
    pub fn any(&self) -> bool {
        *self != Overlays::NONE
    }
}

/// What No Hurt Cam wants of the camera.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Camera {
    /// Factor for the damage tilt (`strength / 100`).
    pub hurt_tilt: f64,
    /// No view bobbing while hurt.
    pub no_bobbing: bool,
}

impl Camera {
    pub const NONE: Camera = Camera {
        hurt_tilt: 1.0,
        no_bobbing: false,
    };
}

static CAMERA: Mutex<Camera> = Mutex::new(Camera::NONE);

pub fn set_camera(c: Camera) {
    if let Ok(mut g) = CAMERA.lock() {
        *g = c;
    }
}

pub fn camera() -> Camera {
    CAMERA.lock().map_or(Camera::NONE, |g| *g)
}

static HIDE_CROSSHAIR: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Custom Crosshair draws its own: Vanilla's crosshair draws are dropped.
pub fn set_hide_crosshair(on: bool) {
    HIDE_CROSSHAIR.store(on, std::sync::atomic::Ordering::Relaxed);
}

pub fn hide_crosshair() -> bool {
    HIDE_CROSSHAIR.load(std::sync::atomic::Ordering::Relaxed)
}

static OVERLAYS: Mutex<Overlays> = Mutex::new(Overlays::NONE);
static BUSY: Mutex<bool> = Mutex::new(false);

pub fn set_overlays(o: Overlays) {
    if let Ok(mut g) = OVERLAYS.lock() {
        *g = o;
    }
}

pub fn overlays() -> Overlays {
    OVERLAYS.lock().map_or(Overlays::NONE, |g| *g)
}

/// Marks the hook busy while it runs the original function; false if it
/// already is (the hook then lets the original run).
pub fn enter() -> bool {
    match BUSY.lock() {
        Ok(mut b) if !*b => {
            *b = true;
            true
        }
        _ => false,
    }
}

pub fn leave() {
    if let Ok(mut b) = BUSY.lock() {
        *b = false;
    }
}

/// The fire quad's corner after Low Fire: `translate(0, offset)` then
/// `scale(s, s)`, so `s * p + (0, offset)`; alpha scaled.
pub fn fire_vertex(o: &Overlays, pos: [f32; 2], alpha: f32) -> ([f32; 2], f32) {
    (
        [
            pos[0] * o.fire_scale,
            pos[1] * o.fire_scale + o.fire_offset_y,
        ],
        alpha * o.fire_alpha,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn none_is_vanilla() {
        assert!(!Overlays::NONE.any());
        let (p, a) = fire_vertex(&Overlays::NONE, [0.5, -0.25], 0.9);
        assert_eq!((p, a), ([0.5, -0.25], 0.9));
    }

    #[test]
    fn fire_moves_down_after_scaling() {
        let o = Overlays {
            fire_offset_y: -0.6,
            fire_scale: 0.5,
            fire_alpha: 0.5,
            ..Overlays::NONE
        };
        assert!(o.any());
        let (p, a) = fire_vertex(&o, [1.0, 1.0], 0.8);
        assert!((p[0] - 0.5).abs() < 1e-6 && (p[1] + 0.1).abs() < 1e-6);
        assert!((a - 0.4).abs() < 1e-6);
    }

    #[test]
    fn busy_guard_is_exclusive() {
        // one test touches the lock, so it cannot race with others
        assert!(enter());
        assert!(!enter());
        leave();
        assert!(enter());
        leave();
    }
}
