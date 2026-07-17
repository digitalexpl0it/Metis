//! Window motion effects (minimize genie, shared animation toggle).

use metis_grid::PixelRect;
use smithay::utils::{Logical, Point};

/// Whether compositor window animations are enabled (`bar.json`), suppressed in
/// VM compatibility graphics mode.
pub fn animations_enabled() -> bool {
    if metis_config::session_graphics_compatibility() {
        return false;
    }
    metis_config::load_bar_config().window_animations
}

pub const MINIMIZE_GENIE_SECS: f32 = 0.48;

/// In-flight minimize genie toward the edge bar.
pub struct MinimizeGenieFx {
    pub started: std::time::Instant,
    /// Visual bounds at animation start (client + SSD chrome when present).
    pub anchor: PixelRect,
    pub target: Point<i32, Logical>,
}

impl MinimizeGenieFx {
    pub fn progress(&self) -> f32 {
        (self.started.elapsed().as_secs_f32() / MINIMIZE_GENIE_SECS).clamp(0.0, 1.0)
    }

    pub fn finished(&self) -> bool {
        self.started.elapsed().as_secs_f32() >= MINIMIZE_GENIE_SECS
    }

    /// Genie-style shrink + travel toward the bar. Returns `(clip rect, alpha)`.
    pub fn frame(&self) -> (PixelRect, f32) {
        let t = ease_in_genie(self.progress());
        let alpha = (1.0 - t * t).clamp(0.0, 1.0);
        let squeeze = t * t;
        let w = lerp_i32(self.anchor.width.max(1), 6, squeeze).max(1);
        let h = lerp_i32(self.anchor.height.max(1), 4, t).max(1);
        let cx = lerp_i32(
            self.anchor.x + self.anchor.width / 2,
            self.target.x,
            t,
        );
        let cy = lerp_i32(
            self.anchor.y + self.anchor.height / 2,
            self.target.y,
            t,
        );
        (
            PixelRect {
                x: cx - w / 2,
                y: cy - h / 2,
                width: w,
                height: h,
            },
            alpha,
        )
    }
}

fn ease_in_genie(t: f32) -> f32 {
    // Fast start, strong pull into the bar at the end (genie “suck”).
    let t = t.clamp(0.0, 1.0);
    1.0 - (1.0 - t).powi(3)
}

fn lerp_i32(a: i32, b: i32, t: f32) -> i32 {
    (a as f32 + (b - a) as f32 * t).round() as i32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn genie_finishes_at_one() {
        let fx = MinimizeGenieFx {
            started: std::time::Instant::now()
                - std::time::Duration::from_secs_f32(MINIMIZE_GENIE_SECS + 0.01),
            anchor: PixelRect {
                x: 0,
                y: 0,
                width: 800,
                height: 600,
            },
            target: Point::from((400, 20)),
        };
        assert!(fx.finished());
        let (rect, alpha) = fx.frame();
        assert!(alpha <= 0.05);
        assert!(rect.width <= 8);
    }
}
