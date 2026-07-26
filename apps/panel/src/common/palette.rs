use eframe::egui::Color32;

pub fn bg_deep(night: bool) -> Color32 {
    if night {
        Color32::from_rgb(9, 9, 11)
    } else {
        Color32::from_rgb(240, 240, 245)
    }
}

pub fn bg_surface(night: bool) -> Color32 {
    if night {
        Color32::from_rgb(24, 24, 28)
    } else {
        Color32::from_rgb(255, 255, 255)
    }
}

pub const ACCENT: Color32 = Color32::from_rgb(102, 178, 255);
pub const DANGER: Color32 = Color32::from_rgb(239, 68, 68);
pub const SUCCESS: Color32 = Color32::from_rgb(34, 197, 94);
pub const WARNING: Color32 = Color32::from_rgb(234, 179, 8);
pub const INFO: Color32 = Color32::from_rgb(14, 165, 233);
pub fn primary(_night: bool) -> Color32 {
    ACCENT
}
pub fn shadow(night: bool) -> Color32 {
    if night {
        Color32::from_rgba_premultiplied(0, 0, 0, 100)
    } else {
        Color32::from_rgba_premultiplied(100, 100, 100, 40)
    }
}

pub fn text_dim(night: bool) -> Color32 {
    if night {
        Color32::from_rgb(160, 160, 170)
    } else {
        Color32::from_rgb(100, 100, 110)
    }
}

pub fn text_bright(night: bool) -> Color32 {
    if night {
        Color32::from_rgb(240, 240, 250)
    } else {
        Color32::from_rgb(20, 20, 30)
    }
}

pub fn border(night: bool) -> Color32 {
    if night {
        Color32::from_rgb(60, 60, 70)
    } else {
        Color32::from_rgb(200, 200, 210)
    }
}

pub const TAG_BG: Color32 = Color32::from_rgba_premultiplied(102, 178, 255, 30);
