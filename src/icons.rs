//! Procedural tray icons.
//!
//! tray-icon accepts raw RGBA, so there is no GDI bitmap, no HICON and no
//! handle to leak — the icons are just pixel buffers built once at startup.

use crate::model::LockLevel;
use tray_icon::Icon;

const SIZE: u32 = 32;

const GREEN: [u8; 3] = [0x3F, 0xB9, 0x50];
const YELLOW: [u8; 3] = [0xF5, 0xB1, 0x0A];
const RED: [u8; 3] = [0xE0, 0x3A, 0x2F];
const GREY: [u8; 3] = [0x9A, 0x9A, 0x9A];

pub struct IconSet {
    green: Icon,
    yellow: Icon,
    red: Icon,
    grey: Icon,
}

impl IconSet {
    pub fn build() -> Result<Self, tray_icon::BadIcon> {
        Ok(IconSet {
            green: make(GREEN)?,
            yellow: make(YELLOW)?,
            red: make(RED)?,
            grey: make(GREY)?,
        })
    }

    pub fn for_level(&self, level: LockLevel) -> Icon {
        match level {
            LockLevel::None => self.green.clone(),
            LockLevel::Standby => self.yellow.clone(),
            LockLevel::Display => self.red.clone(),
            LockLevel::Unknown => self.grey.clone(),
        }
    }
}

fn make(fill: [u8; 3]) -> Result<Icon, tray_icon::BadIcon> {
    Icon::from_rgba(rounded_square(SIZE, fill), SIZE, SIZE)
}

/// A filled rounded square with a darker rim, so it stays legible on both
/// light and dark taskbars. Anti-aliased via a signed distance field.
fn rounded_square(size: u32, fill: [u8; 3]) -> Vec<u8> {
    let border = darken(fill, 0.55);
    let s = size as f32;
    let half = s / 2.0;
    let margin = s * 0.09;
    let hb = half - margin;
    let radius = s * 0.22;
    let border_w = s * 0.09;

    let mut rgba = Vec::with_capacity((size * size * 4) as usize);
    for y in 0..size {
        for x in 0..size {
            let px = x as f32 + 0.5 - half;
            let py = y as f32 + 0.5 - half;

            let d = rounded_box_sdf(px, py, hb - radius, radius);

            // Coverage: fully inside at d <= -0.5, fully outside at d >= 0.5.
            let alpha = (0.5 - d).clamp(0.0, 1.0);
            // Rim: fill in the interior, border colour approaching the edge.
            let t = ((d + border_w) / border_w).clamp(0.0, 1.0);

            let c = lerp(fill, border, t);
            rgba.extend_from_slice(&[c[0], c[1], c[2], (alpha * 255.0).round() as u8]);
        }
    }
    rgba
}

/// Signed distance to a rounded box centred at the origin.
/// Negative inside, positive outside.
fn rounded_box_sdf(px: f32, py: f32, extent: f32, radius: f32) -> f32 {
    let qx = px.abs() - extent;
    let qy = py.abs() - extent;
    let outside = (qx.max(0.0).powi(2) + qy.max(0.0).powi(2)).sqrt();
    let inside = qx.max(qy).min(0.0);
    outside + inside - radius
}

fn darken(c: [u8; 3], factor: f32) -> [u8; 3] {
    [
        (c[0] as f32 * factor) as u8,
        (c[1] as f32 * factor) as u8,
        (c[2] as f32 * factor) as u8,
    ]
}

fn lerp(a: [u8; 3], b: [u8; 3], t: f32) -> [u8; 3] {
    [
        (a[0] as f32 + (b[0] as f32 - a[0] as f32) * t) as u8,
        (a[1] as f32 + (b[1] as f32 - a[1] as f32) * t) as u8,
        (a[2] as f32 + (b[2] as f32 - a[2] as f32) * t) as u8,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buffer_has_the_right_shape() {
        let px = rounded_square(SIZE, GREEN);
        assert_eq!(px.len(), (SIZE * SIZE * 4) as usize);
    }

    #[test]
    fn centre_is_opaque_fill_and_corners_are_transparent() {
        let px = rounded_square(SIZE, GREEN);
        let at = |x: u32, y: u32| {
            let i = ((y * SIZE + x) * 4) as usize;
            [px[i], px[i + 1], px[i + 2], px[i + 3]]
        };
        let centre = at(SIZE / 2, SIZE / 2);
        assert_eq!(centre[3], 255, "centre should be opaque");
        assert_eq!([centre[0], centre[1], centre[2]], GREEN);
        assert_eq!(at(0, 0)[3], 0, "corner should be transparent");
    }

    #[test]
    fn edge_is_darker_than_the_centre() {
        let px = rounded_square(SIZE, GREEN);
        let at = |x: u32, y: u32| {
            let i = ((y * SIZE + x) * 4) as usize;
            px[i + 1] // green channel
        };
        assert!(at(SIZE / 2, 4) < at(SIZE / 2, SIZE / 2));
    }

    #[test]
    fn every_level_produces_an_icon() {
        let set = IconSet::build().expect("icons should build");
        for level in [
            LockLevel::None,
            LockLevel::Standby,
            LockLevel::Display,
            LockLevel::Unknown,
        ] {
            let _ = set.for_level(level);
        }
    }
}
