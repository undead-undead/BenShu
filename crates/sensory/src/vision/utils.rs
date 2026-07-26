use crate::protocol::SensoryOutput;
use image::DynamicImage;
use regex::Regex;

/// Utilities for parsing coordinates from mixed model outputs.
pub struct CoordinateParser;

impl CoordinateParser {
    /// Parse coordinates and normalize based on image dimensions.
    /// Supports:
    /// - [x, y] (absolute or normalized)
    /// - [ymin, xmin, ymax, xmax] (normalized 0-1000)
    /// - [x1, y1, x2, y2]
    pub fn parse_from_text(res: &str, img_w: u32, img_h: u32) -> Option<SensoryOutput> {
        // 1. Try bounding box pattern [y1, x1, y2, x2] or [x1, y1, x2, y2]
        // Often used by vision models that return normalized 0-1000 boxes.
        let re_bbox = Regex::new(r"\[(\d+),\s*(\d+),\s*(\d+),\s*(\d+)\]").unwrap();
        if let Some(caps) = re_bbox.captures(res) {
            let v1 = caps[1].parse::<f32>().unwrap_or(0.0);
            let v2 = caps[2].parse::<f32>().unwrap_or(0.0);
            let v3 = caps[3].parse::<f32>().unwrap_or(0.0);
            let v4 = caps[4].parse::<f32>().unwrap_or(0.0);

            // Heuristic for [ymin, xmin, ymax, xmax] normalized to 1000.
            // If values are <= 1000 and the image is larger, it's likely normalized.
            let is_normalized = v1 <= 1000.0
                && v2 <= 1000.0
                && v3 <= 1000.0
                && v4 <= 1000.0
                && (img_w > 1000 || img_h > 1000);

            let (center_x, center_y) = if is_normalized {
                // GPT style: [ymin, xmin, ymax, xmax]
                let ymin = v1 / 1000.0 * img_h as f32;
                let xmin = v2 / 1000.0 * img_w as f32;
                let ymax = v3 / 1000.0 * img_h as f32;
                let xmax = v4 / 1000.0 * img_w as f32;
                ((xmin + xmax) / 2.0, (ymin + ymax) / 2.0)
            } else {
                // Assume [x1, y1, x2, y2] absolute
                ((v1 + v3) / 2.0, (v2 + v4) / 2.0)
            };

            return Some(SensoryOutput::Coordinates {
                x: center_x,
                y: center_y,
                label: Some(res.to_string()),
            });
        }

        // 2. Try point pattern [x, y]
        let re_point = Regex::new(r"\[(\d+),\s*(\d+)\]").unwrap();
        if let Some(caps) = re_point.captures(res) {
            let x = caps[1].parse::<f32>().unwrap_or(0.0);
            let y = caps[2].parse::<f32>().unwrap_or(0.0);

            // Heuristic for normalization (LLaVA-Grounding often uses 100 or 1000)
            if x <= 100.0 && y <= 100.0 && (img_w > 100 || img_h > 100) {
                return Some(SensoryOutput::Coordinates {
                    x: x / 100.0 * img_w as f32,
                    y: y / 100.0 * img_h as f32,
                    label: Some(res.to_string()),
                });
            }

            return Some(SensoryOutput::Coordinates {
                x,
                y,
                label: Some(res.to_string()),
            });
        }

        None
    }
}
