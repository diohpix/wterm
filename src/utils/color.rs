use eframe::egui;

// ANSI 256색 인덱스를 RGB로 변환 - macOS Terminal 호환
pub fn ansi_256_to_rgb(color_idx: u8) -> egui::Color32 {
    match color_idx {
        // Standard colors (0-15) - macOS Terminal compatible colors
        0 => egui::Color32::from_rgb(0, 0, 0),        // Black
        1 => egui::Color32::from_rgb(194, 54, 33),    // Red
        2 => egui::Color32::from_rgb(37, 188, 36),    // Green
        3 => egui::Color32::from_rgb(173, 173, 39),   // Yellow
        4 => egui::Color32::from_rgb(73, 46, 225),    // Blue
        5 => egui::Color32::from_rgb(211, 56, 211),   // Magenta
        6 => egui::Color32::from_rgb(51, 187, 200),   // Cyan
        7 => egui::Color32::from_rgb(203, 204, 205),  // White
        8 => egui::Color32::from_rgb(129, 131, 131),  // Bright Black (Gray)
        9 => egui::Color32::from_rgb(252, 57, 31),    // Bright Red
        10 => egui::Color32::from_rgb(49, 231, 34),   // Bright Green
        11 => egui::Color32::from_rgb(234, 236, 35),  // Bright Yellow
        12 => egui::Color32::from_rgb(88, 51, 255),   // Bright Blue
        13 => egui::Color32::from_rgb(249, 53, 248),  // Bright Magenta
        14 => egui::Color32::from_rgb(20, 240, 240),  // Bright Cyan
        15 => egui::Color32::from_rgb(233, 235, 235), // Bright White
        // 216 color cube (16-231)
        16..=231 => {
            let idx = color_idx - 16;
            let r = (idx / 36) % 6;
            let g = (idx / 6) % 6;
            let b = idx % 6;
            let r = if r == 0 { 0 } else { 55 + r * 40 };
            let g = if g == 0 { 0 } else { 55 + g * 40 };
            let b = if b == 0 { 0 } else { 55 + b * 40 };
            egui::Color32::from_rgb(r, g, b)
        }
        // Grayscale (232-255)
        232..=255 => {
            let gray = 8 + (color_idx - 232) * 10;
            egui::Color32::from_rgb(gray, gray, gray)
        }
    }
}
