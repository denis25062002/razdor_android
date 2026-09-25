use std::cell::RefCell;

use macroquad::prelude::*;

pub const INK: Color = Color::new(0.93, 0.90, 0.82, 1.0);
pub const DIM: Color = Color::new(0.65, 0.62, 0.55, 1.0);
pub const ACCENT: Color = Color::new(0.95, 0.78, 0.30, 1.0);
pub const PANEL: Color = Color::new(0.12, 0.11, 0.10, 0.92);

thread_local! {
    /// A TrueType font with Cyrillic, for text the built-in pixel font cannot draw (the
    /// original's names and descriptions).
    static FONT: RefCell<Option<Font>> = const { RefCell::new(None) };
}

/// Font files tried for non-ASCII text: `RAZDOR_FONT`, then common system fonts. Nothing is
/// bundled; without one, non-ASCII text is transliterated.
const SYSTEM_FONTS: &[&str] = &[
    "/usr/share/fonts/liberation-sans-fonts/LiberationSans-Regular.ttf",
    "/usr/share/fonts/truetype/liberation/LiberationSans-Regular.ttf",
    "/usr/share/fonts/dejavu-sans-fonts/DejaVuSans.ttf",
    "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
    "/usr/share/fonts/TTF/DejaVuSans.ttf",
    "/usr/share/fonts/noto/NotoSans-Regular.ttf",
    "/usr/share/fonts/truetype/noto/NotoSans-Regular.ttf",
    "/Library/Fonts/Arial Unicode.ttf",
    "/System/Library/Fonts/Supplemental/Arial.ttf",
    "C:\\Windows\\Fonts\\arial.ttf",
];

pub async fn load_font() {
    let candidates = std::env::var("RAZDOR_FONT").into_iter().chain(SYSTEM_FONTS.iter().map(|s| s.to_string()));
    for path in candidates {
        if !std::path::Path::new(&path).exists() {
            continue;
        }
        if let Ok(font) = load_ttf_font(&path).await {
            FONT.with(|f| *f.borrow_mut() = Some(font));
            return;
        }
    }
    eprintln!("no TrueType font with Cyrillic found (set RAZDOR_FONT); transliterating");
}

/// Latin stand-ins for Russian letters, used when no TrueType font is available.
fn transliterate(s: &str) -> String {
    const RU: &str = "абвгдеёжзийклмнопрстуфхцчшщъыьэюя";
    const LAT: [&str; 33] = [
        "a", "b", "v", "g", "d", "e", "e", "zh", "z", "i", "y", "k", "l", "m", "n", "o", "p", "r", "s", "t", "u", "f",
        "kh", "ts", "ch", "sh", "sch", "", "y", "", "e", "yu", "ya",
    ];
    s.chars()
        .map(|c| {
            let lower = c.to_lowercase().next().unwrap_or(c);
            match RU.chars().position(|r| r == lower) {
                Some(i) if c != lower => {
                    let mut t = LAT[i].to_string();
                    if let Some(f) = t.get_mut(0..1) {
                        f.make_ascii_uppercase();
                    }
                    t
                }
                Some(i) => LAT[i].to_string(),
                None if c.is_ascii() => c.to_string(),
                None => "?".to_string(),
            }
        })
        .collect()
}

/// Runs `f` with the font to use for `s` (`None` = the built-in one) and the text to draw.
fn with_font<R>(s: &str, f: impl FnOnce(Option<&Font>, &str) -> R) -> R {
    if s.is_ascii() {
        return f(None, s);
    }
    FONT.with(|font| match font.borrow().as_ref() {
        Some(font) => f(Some(font), s),
        None => f(None, &transliterate(s)),
    })
}

pub fn measure(s: &str, size: f32) -> TextDimensions {
    with_font(s, |font, s| measure_text(s, font, size as u16, 1.0))
}

thread_local! {
    /// A modal dialog is open: the screen below draws but takes no input.
    static BLOCKED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

pub fn set_input_blocked(blocked: bool) {
    BLOCKED.with(|b| b.set(blocked));
}

pub fn input_blocked() -> bool {
    BLOCKED.with(|b| b.get())
}

pub fn mouse_in(x: f32, y: f32, w: f32, h: f32) -> bool {
    let (mx, my) = mouse_position();
    !input_blocked() && mx >= x && mx < x + w && my >= y && my < y + h
}

pub fn clicked() -> bool {
    !input_blocked() && is_mouse_button_pressed(MouseButton::Left)
}

pub fn right_clicked() -> bool {
    !input_blocked() && is_mouse_button_pressed(MouseButton::Right)
}

pub fn key(k: KeyCode) -> bool {
    !input_blocked() && is_key_pressed(k)
}

/// Mouse wheel steps this frame (up is positive), 0 while input is blocked.
pub fn wheel() -> f32 {
    if input_blocked() {
        0.0
    } else {
        mouse_wheel().1
    }
}

/// A translucent panel of lines next to the mouse.
pub fn tooltip(lines: &[(String, Color)]) {
    if lines.is_empty() {
        return;
    }
    let w = lines.iter().map(|(s, _)| measure(s, 17.0).width).fold(0.0, f32::max) + 24.0;
    let h = lines.len() as f32 * 21.0 + 14.0;
    let (mx, my) = mouse_position();
    let x = (mx + 18.0).min(screen_width() - w - 4.0);
    let y = (my + 18.0).min(screen_height() - h - 4.0);
    draw_rectangle(x, y, w, h, Color::new(0.06, 0.12, 0.09, 0.95));
    draw_rectangle_lines(x, y, w, h, 2.0, Color::new(0.35, 0.55, 0.4, 1.0));
    for (i, (s, c)) in lines.iter().enumerate() {
        text(s, x + 12.0, y + 24.0 + i as f32 * 21.0, 17.0, *c);
    }
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
    let dim = measure(label, 22.0);
    text(label, x + (w - dim.width) / 2.0, y + (h + dim.offset_y) / 2.0 - 2.0, 22.0, if enabled { INK } else { DIM });
    let pressed = hover && clicked();
    if pressed {
        super::audio::cue(super::audio::Cue::Button);
    }
    pressed
}

pub fn text(s: &str, x: f32, y: f32, size: f32, color: Color) {
    with_font(s, |font, s| {
        draw_text_ex(s, x, y, TextParams { font, font_size: size as u16, color, ..Default::default() });
    });
}

pub fn text_centered(s: &str, cx: f32, y: f32, size: f32, color: Color) {
    let dim = measure(s, size);
    text(s, cx - dim.width / 2.0, y, size, color);
}

/// Splits `s` into lines no wider than `width` at `size`.
pub fn wrap(s: &str, width: f32, size: f32) -> Vec<String> {
    let mut lines = Vec::new();
    for para in s.split('\n') {
        let mut line = String::new();
        for word in para.split_whitespace() {
            let candidate = if line.is_empty() { word.to_string() } else { format!("{line} {word}") };
            if !line.is_empty() && measure(&candidate, size).width > width {
                lines.push(std::mem::replace(&mut line, word.to_string()));
            } else {
                line = candidate;
            }
        }
        if !line.is_empty() {
            lines.push(line);
        }
    }
    lines
}

pub fn hp_bar(x: f32, y: f32, w: f32, hp: i32, max: i32) {
    let frac = (hp.max(0) as f32 / max as f32).clamp(0.0, 1.0);
    draw_rectangle(x, y, w, 5.0, Color::new(0.25, 0.05, 0.05, 1.0));
    let c = if frac > 0.5 { GREEN } else if frac > 0.25 { YELLOW } else { RED };
    draw_rectangle(x, y, w * frac, 5.0, c);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transliterates_russian() {
        assert_eq!(transliterate("Привет, Мир"), "Privet, Mir");
        assert_eq!(transliterate("ok 12"), "ok 12");
    }
}
