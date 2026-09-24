use macroquad::prelude::*;

pub const INK: Color = Color::new(0.93, 0.90, 0.82, 1.0);
pub const DIM: Color = Color::new(0.65, 0.62, 0.55, 1.0);
pub const ACCENT: Color = Color::new(0.95, 0.78, 0.30, 1.0);
pub const PANEL: Color = Color::new(0.12, 0.11, 0.10, 0.92);

pub fn mouse_in(x: f32, y: f32, w: f32, h: f32) -> bool {
    let (mx, my) = mouse_position();
    mx >= x && mx < x + w && my >= y && my < y + h
}

pub fn clicked() -> bool {
    is_mouse_button_pressed(MouseButton::Left)
}

/// Draws a button and returns true when it was clicked this frame.
pub fn button(x: f32, y: f32, w: f32, h: f32, label: &str, enabled: bool) -> bool {
    let hover = enabled && mouse_in(x, y, w, h);
    let bg = match (enabled, hover) {
        (false, _) => Color::new(0.2, 0.2, 0.2, 1.0),
        (true, true) => Color::new(0.45, 0.35, 0.2, 1.0),
        (true, false) => Color::new(0.3, 0.24, 0.15, 1.0),
    };
    draw_rectangle(x, y, w, h, bg);
    draw_rectangle_lines(x, y, w, h, 2.0, if enabled { ACCENT } else { DIM });
    let dim = measure_text(label, None, 22, 1.0);
    draw_text(label, x + (w - dim.width) / 2.0, y + (h + dim.offset_y) / 2.0 - 2.0, 22.0, if enabled { INK } else { DIM });
    hover && clicked()
}

pub fn text(s: &str, x: f32, y: f32, size: f32, color: Color) {
    draw_text(s, x, y, size, color);
}

pub fn text_centered(s: &str, cx: f32, y: f32, size: f32, color: Color) {
    let dim = measure_text(s, None, size as u16, 1.0);
    draw_text(s, cx - dim.width / 2.0, y, size, color);
}

pub fn hp_bar(x: f32, y: f32, w: f32, hp: i32, max: i32) {
    let frac = (hp.max(0) as f32 / max as f32).clamp(0.0, 1.0);
    draw_rectangle(x, y, w, 5.0, Color::new(0.25, 0.05, 0.05, 1.0));
    let c = if frac > 0.5 { GREEN } else if frac > 0.25 { YELLOW } else { RED };
    draw_rectangle(x, y, w * frac, 5.0, c);
}
