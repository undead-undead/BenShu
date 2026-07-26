use serde::{Deserialize, Serialize};

/// Maps normalized coordinates (0-1000) to physical/viewport pixels.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ViewportScale {
    pub width: u32,
    pub height: u32,
    pub dpi_scale: f32,
}

impl ViewportScale {
    pub fn new(width: u32, height: u32, dpi_scale: f32) -> Self {
        Self {
            width,
            height,
            dpi_scale,
        }
    }

    /// Scale a normalized coordinate (0-1000) to viewport pixels.
    pub fn scale_point(&self, nx: u32, ny: u32) -> (i32, i32) {
        let x = (nx as f32 / 1000.0 * self.width as f32) as i32;
        let y = (ny as f32 / 1000.0 * self.height as f32) as i32;
        (x, y)
    }

    /// Convert viewport pixels to physical pixels (DPI aware).
    pub fn to_physical(&self, vx: i32, vy: i32) -> (i32, i32) {
        (
            (vx as f32 * self.dpi_scale) as i32,
            (vy as f32 * self.dpi_scale) as i32,
        )
    }

    /// Map normalized [0-1000] detection box to physical [x, y, w, h].
    pub fn map_box(&self, norm_box: [u32; 4]) -> [i32; 4] {
        let (x, y) = self.scale_point(norm_box[0], norm_box[1]);
        let (w, h) = self.scale_point(norm_box[2], norm_box[3]);
        let (px, py) = self.to_physical(x, y);
        let (pw, ph) = self.to_physical(w, h);
        [px, py, pw, ph]
    }
}
