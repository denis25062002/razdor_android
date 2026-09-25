//! The original's window art: marble and parchment surfaces, title bars, silver frames,
//! buttons, card frames, empty-cell icons, resource icons and the battle's spell
//! animations. Everything is decoded at runtime from the player's install
//! (`Graphics/Windows`, `Graphics/Spells`, `Graphics/Battle`) and cached as textures; nothing
//! decoded is written anywhere. Without an install each piece falls back to our own
//! placeholder style with the same layout.
//!
//! Layout numbers of the screens that follow the original closely (battle, bottom bar) are
//! in pixels of the 960×720 gameplay video (`docs/reference/video-notes.md`), scaled by [`k`].

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;

use macroquad::prelude::*;

use razdor::dt::gfx::{self, Image};
use razdor::dt::ini::Ini;
use razdor::dt::install::{find_path, SETTINGS_FILE};

use super::widgets::{measure, text};

/// How a decoded picture is turned into a texture.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Fx {
    /// As stored.
    Plain,
    /// Black is transparent (frames drawn over a black middle).
    KeyBlack,
    /// Additive art (spell animations, glyph icons): brightness becomes alpha.
    Glow,
    /// Greyed out (disabled buttons).
    Grey,
    /// A dark overlay whose opacity is the picture's brightness (ornaments on parchment).
    Shade,
}

struct Chrome {
    dir: PathBuf,
    /// By path, then by [`Fx`] (its index).
    textures: HashMap<String, [Option<Option<Texture2D>>; 5]>,
    animations: HashMap<String, Option<Vec<Texture2D>>>,
    texts: Option<Option<Ini>>,
}

thread_local! {
    static CHROME: RefCell<Option<Chrome>> = const { RefCell::new(None) };
}

/// Use the art of the install in `dir` from now on.
pub fn set_install(dir: PathBuf) {
    CHROME.with(|c| *c.borrow_mut() = Some(Chrome { dir, textures: HashMap::new(), animations: HashMap::new(), texts: None }));
}

// ------------------------------------------------------------------------------------------
// Pixel transforms (pure)
// ------------------------------------------------------------------------------------------

fn lum(p: &[u8]) -> u32 {
    (p[0] as u32 * 30 + p[1] as u32 * 59 + p[2] as u32 * 11) / 100
}

/// Applies `fx` to `img` in place.
pub fn transform(img: &mut Image, fx: Fx) {
    for p in img.rgba.chunks_exact_mut(4) {
        match fx {
            Fx::Plain => {}
            Fx::KeyBlack => {
                let m = p[0].max(p[1]).max(p[2]) as u32;
                let a = (m.saturating_sub(10) * 16).min(255);
                p[3] = p[3].min(a as u8);
            }
            Fx::Glow => {
                let m = p[0].max(p[1]).max(p[2]);
                if m == 0 {
                    p[3] = 0;
                } else {
                    for c in &mut p[..3] {
                        *c = (*c as u32 * 255 / m as u32) as u8;
                    }
                    p[3] = m;
                }
            }
            Fx::Grey => {
                let l = (lum(p) * 9 / 10) as u8;
                p[0] = l;
                p[1] = l;
                p[2] = l;
            }
            Fx::Shade => {
                let a = lum(p) as u8;
                p[..3].copy_from_slice(&[20, 10, 4]);
                p[3] = a.min(p[3]);
            }
        }
    }
}

fn to_texture(img: &Image) -> Option<Texture2D> {
    let (w, h) = (u16::try_from(img.width).ok()?, u16::try_from(img.height).ok()?);
    let t = Texture2D::from_rgba8(w, h, &img.rgba);
    t.set_filter(FilterMode::Linear);
    Some(t)
}

/// Picture `rel` under `Graphics/` (e.g. `"Windows/Win-red.lit"`), first frame, with `fx`.
/// `None` without an install or when the file is missing (logged once).
pub fn art(rel: &str, fx: Fx) -> Option<Texture2D> {
    CHROME.with(|c| {
        let mut c = c.borrow_mut();
        let c = c.as_mut()?;
        if let Some(t) = c.textures.get(rel).and_then(|v| v[fx as usize].as_ref()) {
            return t.clone();
        }
        let t = find_path(&c.dir, &format!("Graphics/{rel}"))
            .and_then(|p| gfx::decode_file(&p))
            .map_err(|e| eprintln!("Discord Times art: {rel}: {e}"))
            .ok()
            .and_then(|mut frames| {
                let img = frames.get_mut(0)?;
                transform(img, fx);
                to_texture(img)
            });
        c.textures.entry(rel.to_string()).or_default()[fx as usize] = Some(t.clone());
        t
    })
}

/// Windows art (`Graphics/Windows/<name>.lit`).
pub fn win(name: &str) -> Option<Texture2D> {
    art(&format!("Windows/{name}.lit"), Fx::Plain)
}

pub fn win_fx(name: &str, fx: Fx) -> Option<Texture2D> {
    art(&format!("Windows/{name}.lit"), fx)
}

/// Every frame of an animation (`Graphics/<rel>`); additive ones are made to glow.
pub fn animation(rel: &str) -> Option<Vec<Texture2D>> {
    CHROME.with(|c| {
        let mut c = c.borrow_mut();
        let c = c.as_mut()?;
        if let Some(t) = c.animations.get(rel) {
            return t.clone();
        }
        let t = find_path(&c.dir, &format!("Graphics/{rel}"))
            .and_then(|p| gfx::decode_file(&p))
            .map_err(|e| eprintln!("Discord Times art: {rel}: {e}"))
            .ok()
            .map(|mut frames| {
                let additive = gfx::is_additive(&frames);
                frames
                    .iter_mut()
                    .filter_map(|f| {
                        if additive {
                            transform(f, Fx::Glow);
                        }
                        to_texture(f)
                    })
                    .collect::<Vec<_>>()
            })
            .filter(|v| !v.is_empty());
        c.animations.insert(rel.to_string(), t.clone());
        t
    })
}

/// An interface text of the install (`Rus_DiscordTimes.ini`), e.g. `("Army", "Bonus2")`.
pub fn ui_text(section: &str, key: &str) -> Option<String> {
    CHROME.with(|c| {
        let mut c = c.borrow_mut();
        let c = c.as_mut()?;
        if c.texts.is_none() {
            c.texts = Some(read_texts(&c.dir));
        }
        text_of(c.texts.as_ref()?.as_ref()?, section, key)
    })
}

fn read_texts(dir: &std::path::Path) -> Option<Ini> {
    find_path(dir, SETTINGS_FILE).ok().and_then(|p| std::fs::read(p).ok()).map(|b| Ini::from_cp1251(&b))
}

/// A text of the ini; the original's `#\` line breaks become newlines.
fn text_of(ini: &Ini, section: &str, key: &str) -> Option<String> {
    ini.section(section)?.get_nonempty(key).map(|s| s.replace("#\\", "\n"))
}

#[cfg(test)]
fn ui_text_from(dir: &std::path::Path, section: &str, key: &str) -> Option<String> {
    text_of(&read_texts(dir)?, section, key)
}

// ------------------------------------------------------------------------------------------
// Geometry and plain drawing
// ------------------------------------------------------------------------------------------

/// Screen pixels per pixel of the 960×720 reference video (the original ran at 1024×768).
pub fn k() -> f32 {
    ((screen_width() / 1024.0).min(screen_height() / 768.0) / 0.9375).max(0.5)
}

/// Height of the bottom game bar.
pub fn bar_height() -> f32 {
    (80.0 * k()).round()
}

pub fn tex(t: &Texture2D, r: Rect, color: Color) {
    draw_texture_ex(t, r.x, r.y, color, DrawTextureParams { dest_size: Some(vec2(r.w, r.h)), ..Default::default() });
}

pub fn tex_src(t: &Texture2D, src: Rect, dst: Rect, color: Color) {
    draw_texture_ex(t, dst.x, dst.y, color, DrawTextureParams { dest_size: Some(vec2(dst.w, dst.h)), source: Some(src), ..Default::default() });
}

/// Repeats `t` over `r` at `scale` screen pixels per texel, clipping the last row and column.
pub fn tile(t: &Texture2D, r: Rect, scale: f32, color: Color) {
    let (tw, th) = (t.width() * scale, t.height() * scale);
    if tw < 1.0 || th < 1.0 {
        return;
    }
    let mut y = r.y;
    while y < r.y + r.h - 0.5 {
        let h = th.min(r.y + r.h - y);
        let mut x = r.x;
        while x < r.x + r.w - 0.5 {
            let w = tw.min(r.x + r.w - x);
            tex_src(t, Rect::new(0.0, 0.0, w / scale, h / scale), Rect::new(x, y, w, h), color);
            x += tw;
        }
        y += th;
    }
}

/// Three-slice: the ends of `t` keep their aspect, the middle stretches to `r.w`.
pub fn three_slice(t: &Texture2D, r: Rect, end: f32, color: Color) {
    let s = r.h / t.height();
    let e = (end * s).min(r.w / 2.0);
    let src_e = e / s;
    tex_src(t, Rect::new(0.0, 0.0, src_e, t.height()), Rect::new(r.x, r.y, e, r.h), color);
    tex_src(t, Rect::new(src_e, 0.0, t.width() - 2.0 * src_e, t.height()), Rect::new(r.x + e, r.y, r.w - 2.0 * e, r.h), color);
    tex_src(t, Rect::new(t.width() - src_e, 0.0, src_e, t.height()), Rect::new(r.x + r.w - e, r.y, e, r.h), color);
}

/// Text with the original's dark drop shadow.
pub fn shadow_text(s: &str, x: f32, y: f32, size: f32, color: Color) {
    let o = (size / 14.0).clamp(1.0, 2.0);
    text(s, x + o, y + o, size, Color::new(0.0, 0.0, 0.0, 0.75 * color.a));
    text(s, x, y, size, color);
}

/// Text with a heavier dark edge, readable over a picture (the unit panel's stats over the
/// figure).
pub fn strong_text(s: &str, x: f32, y: f32, size: f32, color: Color) {
    let dark = Color::new(0.08, 0.04, 0.0, 0.8 * color.a);
    for (dx, dy) in [(1.0, 1.0), (1.0, 0.0), (0.0, 1.0), (-0.6, 0.0), (0.0, -0.6)] {
        text(s, x + dx, y + dy, size, dark);
    }
    text(s, x, y, size, color);
}

pub fn strong_centered(s: &str, cx: f32, y: f32, size: f32, color: Color) {
    let w = measure(s, size).width;
    strong_text(s, cx - w / 2.0, y, size, color);
}

pub fn strong_right(s: &str, rx: f32, y: f32, size: f32, color: Color) {
    let w = measure(s, size).width;
    strong_text(s, rx - w, y, size, color);
}

pub fn shadow_centered(s: &str, cx: f32, y: f32, size: f32, color: Color) {
    let w = measure(s, size).width;
    shadow_text(s, cx - w / 2.0, y, size, color);
}

pub fn shadow_right(s: &str, rx: f32, y: f32, size: f32, color: Color) {
    let w = measure(s, size).width;
    shadow_text(s, rx - w, y, size, color);
}

// ------------------------------------------------------------------------------------------
// Surfaces and frames
// ------------------------------------------------------------------------------------------

/// Palette of the placeholder style (and of text drawn over the original's art).
pub const CREAM: Color = Color::new(0.96, 0.91, 0.76, 1.0);
pub const GOLD: Color = Color::new(1.0, 0.82, 0.42, 1.0);
pub const SILVER: Color = Color::new(0.80, 0.80, 0.84, 1.0);
pub const SILVER_DARK: Color = Color::new(0.30, 0.31, 0.34, 1.0);
pub const BLUE_TEXT: Color = Color::new(0.45, 0.70, 1.0, 1.0);
pub const RED_TEXT: Color = Color::new(1.0, 0.33, 0.25, 1.0);
pub const ORANGE_TEXT: Color = Color::new(1.0, 0.70, 0.25, 1.0);

/// A window's or panel's background.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Skin {
    /// Green marble: most windows, tooltips.
    Marble,
    /// Red marble: the battle.
    Red,
    /// Light parchment: building tabs, the unit panel.
    Paper,
    /// Red-brown parchment: story and description text boxes.
    Brown,
    /// Gold-brown: the stat strip under a card.
    Strip,
}

impl Skin {
    fn texture(self) -> Option<(Texture2D, Color)> {
        let paper = |c: Color| win("Win-paper").map(|t| (t, c));
        match self {
            Skin::Marble => win("Win-marble").map(|t| (t, WHITE)),
            Skin::Red => win("Win-red").map(|t| (t, WHITE)),
            Skin::Paper => paper(WHITE),
            Skin::Brown => paper(Color::new(0.50, 0.25, 0.14, 1.0)),
            Skin::Strip => paper(Color::new(0.58, 0.40, 0.18, 1.0)),
        }
    }

    /// The placeholder colour.
    pub fn color(self) -> Color {
        match self {
            Skin::Marble => Color::new(0.10, 0.22, 0.18, 1.0),
            Skin::Red => Color::new(0.30, 0.06, 0.04, 1.0),
            Skin::Paper => Color::new(0.86, 0.68, 0.45, 1.0),
            Skin::Brown => Color::new(0.38, 0.18, 0.10, 1.0),
            Skin::Strip => Color::new(0.45, 0.30, 0.12, 1.0),
        }
    }
}

/// Brightness (0..255) of a tileable cloudy noise, `n`×`n`: our own stand-in for the
/// original's marble and parchment (pure, deterministic).
pub fn marble_noise(n: usize) -> Vec<u8> {
    let hash = |x: usize, y: usize, s: u32| -> f32 {
        let mut h = (x as u32).wrapping_mul(374_761_393) ^ (y as u32).wrapping_mul(668_265_263) ^ s.wrapping_mul(2_246_822_519);
        h = (h ^ (h >> 13)).wrapping_mul(1_274_126_177);
        ((h ^ (h >> 16)) & 0xffff) as f32 / 65535.0
    };
    let mut out = vec![0f32; n * n];
    let mut amp = 1.0;
    let mut total = 0.0;
    for (octave, cells) in [4usize, 8, 16, 32].into_iter().enumerate() {
        let step = n as f32 / cells as f32;
        for y in 0..n {
            for x in 0..n {
                let (fx, fy) = (x as f32 / step, y as f32 / step);
                let (x0, y0) = (fx.floor() as usize, fy.floor() as usize);
                let (tx, ty) = (fx - x0 as f32, fy - y0 as f32);
                let (sx, sy) = (tx * tx * (3.0 - 2.0 * tx), ty * ty * (3.0 - 2.0 * ty));
                let v = |i: usize, j: usize| hash(i % cells, j % cells, octave as u32 + 1);
                let top = v(x0, y0) + (v(x0 + 1, y0) - v(x0, y0)) * sx;
                let bottom = v(x0, y0 + 1) + (v(x0 + 1, y0 + 1) - v(x0, y0 + 1)) * sx;
                out[y * n + x] += amp * (top + (bottom - top) * sy);
            }
        }
        total += amp;
        amp *= 0.55;
    }
    out.iter().map(|v| (150.0 + 105.0 * (v / total).clamp(0.0, 1.0)) as u8).collect()
}

thread_local! {
    static NOISE: RefCell<Option<Texture2D>> = const { RefCell::new(None) };
}

fn noise_texture() -> Texture2D {
    NOISE.with(|t| {
        t.borrow_mut()
            .get_or_insert_with(|| {
                let n = 128;
                let rgba: Vec<u8> = marble_noise(n).into_iter().flat_map(|v| [v, v, v, 255]).collect();
                let tex = Texture2D::from_rgba8(n as u16, n as u16, &rgba);
                tex.set_filter(FilterMode::Linear);
                tex
            })
            .clone()
    })
}

/// Fills `r` with `skin`: the original's texture, or our cloudy placeholder in the skin's
/// colour.
pub fn surface(r: Rect, skin: Skin) {
    if let Some((t, c)) = skin.texture() {
        tile(&t, r, k().min(1.4), c);
        return;
    }
    let base = skin.color();
    let lift = |v: f32| (v * 1.25).min(1.0);
    tile(&noise_texture(), r, 2.0, Color::new(lift(base.r), lift(base.g), lift(base.b), 1.0));
}

/// The silver edge of windows and panels (outer light line, inner dark line).
pub fn silver_frame(r: Rect, width: f32) {
    draw_rectangle_lines(r.x, r.y, r.w, r.h, width, SILVER);
    draw_rectangle_lines(r.x + width, r.y + width, r.w - 2.0 * width, r.h - 2.0 * width, 1.0, SILVER_DARK);
}

/// The window's close box; true when clicked.
pub fn close_button(x: f32, y: f32, s: f32, red: bool) -> bool {
    let hover = super::widgets::mouse_in(x, y, s, s);
    let name = match (red, hover) {
        (true, false) => "CloseButtonRed-Up",
        (true, true) => "CloseButtonRed-Down2",
        (false, false) => "CloseButtonGreen-Up",
        (false, true) => "CloseButtonGreen-Down2",
    };
    if let Some(t) = win(name) {
        tex(&t, Rect::new(x, y, s, s), WHITE);
    } else {
        draw_rectangle(x, y, s, s, Color::new(0.05, 0.08, 0.06, 1.0));
        draw_rectangle_lines(x, y, s, s, 1.5, if hover { GOLD } else { SILVER });
        let c = if red { RED_TEXT } else { Color::new(0.4, 0.9, 0.5, 1.0) };
        let p = s * 0.25;
        draw_line(x + p, y + p, x + s - p, y + s - p, 2.0, c);
        draw_line(x + s - p, y + p, x + p, y + s - p, 2.0, c);
    }
    hover && super::widgets::clicked()
}

/// Title bar of a window: the window's own skin with ornaments at the ends, the title
/// centred. Returns true when its close box (if any) was clicked.
pub fn title_bar(r: Rect, title: &str, skin: Skin, closable: bool) -> bool {
    let k = k();
    surface(r, skin);
    draw_rectangle(r.x, r.y, r.w, r.h, Color::new(0.0, 0.0, 0.0, 0.15));
    // Orange ornaments at both ends of the battle's title.
    if let Some(t) = win_fx("Corner-Left", Fx::KeyBlack).filter(|_| skin == Skin::Red && r.w > 400.0) {
        let tint = Color::new(1.0, 0.45, 0.1, 0.9);
        let h = r.h * 0.8;
        let w = t.width() * h / t.height();
        tex(&t, Rect::new(r.x + 4.0 * k, r.y + (r.h - h) / 2.0, w, h), tint);
        if let Some(t2) = win_fx("Corner-Right", Fx::KeyBlack) {
            let right = r.x + r.w - w - 4.0 * k - if closable { r.h + 4.0 * k } else { 0.0 };
            tex(&t2, Rect::new(right, r.y + (r.h - h) / 2.0, w, h), tint);
        }
    }
    draw_line(r.x, r.y + r.h, r.x + r.w, r.y + r.h, 1.0 * k, SILVER);
    let size = (16.0 * k).round();
    let color = if skin == Skin::Red { GOLD } else { CREAM };
    let mut t = title.to_string();
    let room = if skin == Skin::Red && r.w > 400.0 { r.w - 300.0 * k } else { r.w - 2.0 * r.h - 8.0 * k };
    while measure(&t, size).width > room && t.chars().count() > 4 {
        t.pop();
    }
    shadow_centered(&t, r.x + r.w / 2.0, r.y + r.h * 0.5 + size * 0.36, size, color);
    if closable {
        let s = r.h - 6.0 * k;
        return close_button(r.x + r.w - s - 4.0 * k, r.y + 3.0 * k, s, skin == Skin::Red);
    }
    false
}

/// A modal window: background, silver frame, title bar. Returns the area below the title
/// bar and whether the close box was clicked.
pub fn window(r: Rect, title: &str, skin: Skin, closable: bool) -> (Rect, bool) {
    let k = k();
    draw_rectangle(r.x + 6.0 * k, r.y + 6.0 * k, r.w, r.h, Color::new(0.0, 0.0, 0.0, 0.35));
    surface(r, skin);
    let tb = (26.0 * k).round();
    let closed = title_bar(Rect::new(r.x, r.y, r.w, tb), title, skin, closable);
    silver_frame(r, (1.5 * k).max(1.0));
    (Rect::new(r.x + 2.0 * k, r.y + tb + 1.0, r.w - 4.0 * k, r.h - tb - 3.0 * k), closed)
}

/// A thin ornamental divider (the strip between the battle's formations, without text).
pub fn divider(r: Rect) {
    if let Some(t) = win("HintFrame") {
        three_slice(&t, r, 40.0, WHITE);
    } else {
        draw_rectangle(r.x, r.y, r.w, r.h, Color::new(0.06, 0.04, 0.03, 0.75));
        draw_line(r.x, r.y + 2.0, r.x + r.w, r.y + 2.0, 1.0, SILVER);
        draw_line(r.x, r.y + r.h - 2.0, r.x + r.w, r.y + r.h - 2.0, 1.0, SILVER);
    }
}

/// The divider with a hint in the middle (the battle's "press ESC" strip).
pub fn hint_strip(r: Rect, hint: &str, color: Color) {
    divider(r);
    hint_text(r, hint, color);
}

/// The hint of a strip, in the middle of `r` over a dark band.
pub fn hint_text(r: Rect, hint: &str, color: Color) {
    draw_rectangle(r.x + r.h, r.y + r.h * 0.22, r.w - 2.0 * r.h, r.h * 0.56, Color::new(0.08, 0.03, 0.02, 0.55));
    let size = (r.h * 0.62).round().max(11.0);
    let mut s = hint.to_string();
    while measure(&s, size).width > r.w - 3.0 * r.h && s.chars().count() > 4 {
        s.pop();
    }
    shadow_centered(&s, r.x + r.w / 2.0, r.y + r.h * 0.5 + size * 0.36, size, color);
}

/// Corner ornaments of a text box.
fn corners(r: Rect) {
    let s = (26.0 * k()).min(r.h / 2.0).min(r.w / 2.0);
    for (name, x, y) in [
        ("Corner_Frame-LU", r.x, r.y),
        ("Corner_Frame-RU", r.x + r.w - s, r.y),
        ("Corner_Frame-LD", r.x, r.y + r.h - s),
        ("Corner_Frame-RD", r.x + r.w - s, r.y + r.h - s),
    ] {
        if let Some(t) = win(name) {
            tex(&t, Rect::new(x, y, s, s), WHITE);
        }
    }
}

/// The red-brown parchment box of story and description texts, with silver corners.
pub fn text_box(r: Rect) {
    surface(r, Skin::Brown);
    draw_rectangle_lines(r.x, r.y, r.w, r.h, 1.5, Color::new(0.72, 0.62, 0.5, 1.0));
    corners(r);
}

/// Light parchment, e.g. the unit panel; ornaments darken the corners when the art is there.
pub fn parchment(r: Rect, ornaments: bool) {
    surface(r, Skin::Paper);
    if ornaments {
        if let Some(t) = win_fx("UnitFrame", Fx::Shade) {
            tex(&t, r, Color::new(1.0, 1.0, 1.0, 0.8));
        }
    }
    // The original darkens the parchment towards the edges.
    let e = 14.0 * k();
    let dark = Color::new(0.25, 0.12, 0.02, 0.18);
    draw_rectangle(r.x, r.y, r.w, e, dark);
    draw_rectangle(r.x, r.y + r.h - e, r.w, e, dark);
    draw_rectangle(r.x, r.y + e, e, r.h - 2.0 * e, dark);
    draw_rectangle(r.x + r.w - e, r.y + e, e, r.h - 2.0 * e, dark);
    draw_rectangle_lines(r.x, r.y, r.w, r.h, 1.5, Color::new(0.75, 0.6, 0.3, 1.0));
}

// ------------------------------------------------------------------------------------------
// Buttons
// ------------------------------------------------------------------------------------------

/// A text button on green marble with a silver edge (the original's `Btn*`). True when
/// clicked this frame.
pub fn marble_button(r: Rect, label: &str, enabled: bool, hover: bool) {
    let name = if r.w < 110.0 { "Btn1" } else if r.w < 145.0 { "Btn2" } else { "Btn3" };
    let state = if hover && enabled { "Down" } else { "Up" };
    let fx = if enabled { Fx::Plain } else { Fx::Grey };
    if let Some(t) = win_fx(&format!("{name}{state}"), fx) {
        three_slice(&t, r, 14.0, WHITE);
    } else {
        let bg = match (enabled, hover) {
            (false, _) => Color::new(0.2, 0.22, 0.21, 1.0),
            (true, true) => Color::new(0.16, 0.36, 0.28, 1.0),
            (true, false) => Color::new(0.10, 0.25, 0.19, 1.0),
        };
        draw_rectangle(r.x, r.y, r.w, r.h, bg);
        draw_rectangle_lines(r.x, r.y, r.w, r.h, 2.0, if enabled { SILVER } else { SILVER_DARK });
        draw_rectangle_lines(r.x + 2.0, r.y + 2.0, r.w - 4.0, r.h - 4.0, 1.0, Color::new(0.0, 0.0, 0.0, 0.5));
    }
    let mut size: f32 = (r.h * 0.56).clamp(12.0, 22.0).round();
    while size > 11.0 && measure(label, size).width > r.w - 10.0 {
        size -= 1.0;
    }
    let d = measure(label, size);
    let color = if !enabled {
        Color::new(0.6, 0.6, 0.58, 1.0)
    } else if hover {
        GOLD
    } else {
        CREAM
    };
    shadow_text(label, r.x + (r.w - d.width) / 2.0, r.y + (r.h + d.offset_y) / 2.0 - 1.0, size, color);
}

/// The small pill buttons of the barracks ("Hire" green, "Heal" blue).
pub fn pill_button(r: Rect, label: &str, enabled: bool, green: bool) -> bool {
    let hover = enabled && super::widgets::mouse_in(r.x, r.y, r.w, r.h);
    let name = if !enabled {
        "smb-disable"
    } else if hover {
        "smb-down"
    } else {
        "smb-up"
    };
    let tint = if green && enabled { Color::new(0.35, 1.0, 0.35, 1.0) } else { WHITE };
    match win(name) {
        Some(t) if green && enabled => {
            // The blue pill, recoloured green.
            let g = win_fx(name, Fx::Grey).unwrap_or(t);
            three_slice(&g, r, 12.0, tint);
        }
        Some(t) => three_slice(&t, r, 12.0, WHITE),
        None => {
            let c = match (enabled, green) {
                (false, _) => Color::new(0.35, 0.35, 0.35, 1.0),
                (true, true) => Color::new(0.15, 0.55, 0.15, 1.0),
                (true, false) => Color::new(0.12, 0.25, 0.65, 1.0),
            };
            draw_rectangle(r.x, r.y, r.w, r.h, c);
            draw_rectangle_lines(r.x, r.y, r.w, r.h, 1.5, if hover { GOLD } else { SILVER });
        }
    }
    let size = (r.h * 0.62).round().clamp(11.0, 18.0);
    let d = measure(label, size);
    shadow_text(label, r.x + (r.w - d.width) / 2.0, r.y + (r.h + d.offset_y) / 2.0 - 1.0, size, if enabled { WHITE } else { Color::new(0.8, 0.8, 0.8, 1.0) });
    let pressed = hover && super::widgets::clicked();
    if pressed {
        super::audio::cue(super::audio::Cue::Button);
    }
    pressed
}

// ------------------------------------------------------------------------------------------
// Cards and cells
// ------------------------------------------------------------------------------------------

/// The picture of an empty formation cell: where each kind of unit belongs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CellIcon {
    /// Front row: crossed swords.
    Swords,
    /// Back row: a bow on a shield.
    Bow,
    /// Reserve (the ends of the wide back row, or the reserve row): a tent.
    Tent,
}

impl CellIcon {
    /// The icon of `slot`: the outer columns of a six-wide formation are the tents.
    pub fn of(f: razdor::rules::formation::Formation, slot: razdor::rules::formation::Slot) -> CellIcon {
        use razdor::rules::formation::Row;
        let (_, col) = f.display(slot);
        match slot.row {
            Row::Reserve => CellIcon::Tent,
            _ if f.cols >= 6 && (col == 0 || col + 1 == f.cols) => CellIcon::Tent,
            Row::Front => CellIcon::Swords,
            Row::Back => CellIcon::Bow,
        }
    }

    fn art(self) -> &'static str {
        match self {
            CellIcon::Swords => "wb1",
            CellIcon::Bow => "wb2",
            CellIcon::Tent => "wb3",
        }
    }
}

/// Placeholder drawings of the cell icons, grey like the original's.
fn draw_cell_icon_fallback(icon: CellIcon, r: Rect) {
    let c = Color::new(0.62, 0.62, 0.64, 0.75);
    let d = Color::new(0.25, 0.25, 0.27, 0.8);
    let (cx, cy, s) = (r.x + r.w / 2.0, r.y + r.h / 2.0, r.w.min(r.h));
    match icon {
        CellIcon::Swords => {
            for dir in [-1.0f32, 1.0] {
                let (x0, y0) = (cx - dir * s * 0.34, cy - s * 0.34);
                let (x1, y1) = (cx + dir * s * 0.30, cy + s * 0.30);
                draw_line(x0, y0, x1, y1, s * 0.06, c);
                // Cross guard and pommel.
                let (gx, gy) = (cx + dir * s * 0.18, cy + s * 0.18);
                draw_line(gx - s * 0.08, gy - dir * s * 0.08, gx + s * 0.08, gy + dir * s * 0.08, s * 0.05, d);
                draw_circle(x1, y1, s * 0.04, c);
            }
        }
        CellIcon::Bow => {
            // Shield.
            let (w, h) = (s * 0.62, s * 0.72);
            let (x, y) = (cx - w / 2.0, cy - h / 2.0);
            draw_triangle(vec2(x, y + h * 0.45), vec2(x + w, y + h * 0.45), vec2(cx, y + h), d);
            draw_rectangle(x, y, w, h * 0.46, d);
            draw_rectangle_lines(x, y, w, h * 0.46, 2.0, c);
            draw_line(x, y + h * 0.45, cx, y + h, 2.0, c);
            draw_line(x + w, y + h * 0.45, cx, y + h, 2.0, c);
            // Bow and arrow.
            for i in 0..8 {
                let a0 = -1.1 + i as f32 * 0.275;
                let a1 = a0 + 0.275;
                let rr = s * 0.24;
                draw_line(cx + rr * a0.sin(), cy + rr * a0.cos(), cx + rr * a1.sin(), cy + rr * a1.cos(), 2.0, c);
            }
            draw_line(cx, cy - s * 0.26, cx, cy + s * 0.26, 1.5, c);
        }
        CellIcon::Tent => {
            let (w, h) = (s * 0.62, s * 0.62);
            let (x, y) = (cx - w / 2.0, cy - h / 2.0 + s * 0.04);
            draw_triangle(vec2(cx, y), vec2(x, y + h), vec2(x + w, y + h), d);
            draw_triangle(vec2(cx, y + h * 0.35), vec2(cx - w * 0.14, y + h), vec2(cx + w * 0.14, y + h), Color::new(0.1, 0.1, 0.1, 0.7));
            draw_line(cx, y, x, y + h, 2.0, c);
            draw_line(cx, y, x + w, y + h, 2.0, c);
            draw_line(cx, y, cx, y - s * 0.12, 2.0, c);
            draw_triangle(vec2(cx, y - s * 0.12), vec2(cx + s * 0.14, y - s * 0.09), vec2(cx, y - s * 0.05), c);
        }
    }
}

/// An empty cell: the dark square with its icon and, below it (`ornament`), the silver
/// ornament the original draws under empty cells.
pub fn empty_cell(r: Rect, icon: CellIcon, ornament: bool) {
    let k = k();
    if let Some(t) = win("Shadow-Empty") {
        // The shadow's box is a little larger than the card; its ornament hangs below.
        let sw = r.w * 101.0 / 92.0;
        let sh = sw * 141.0 / 101.0;
        let src_h = if ornament { 141.0 } else { 96.0 };
        tex_src(&t, Rect::new(0.0, 0.0, 101.0, src_h), Rect::new(r.x - 1.0 * k, r.y - 1.0 * k, sw, sh * src_h / 141.0), Color::new(1.0, 1.0, 1.0, 0.55));
    } else {
        draw_rectangle(r.x, r.y, r.w, r.w, Color::new(0.0, 0.0, 0.0, 0.4));
        if ornament {
            let (cx, y) = (r.x + r.w / 2.0, r.y + r.w + 4.0 * k);
            let c = Color::new(0.7, 0.7, 0.72, 0.6);
            draw_triangle(vec2(cx - r.w * 0.38, y), vec2(cx + r.w * 0.38, y), vec2(cx, y + 22.0 * k), Color::new(0.5, 0.5, 0.52, 0.35));
            draw_line(cx - r.w * 0.4, y, cx + r.w * 0.4, y, 2.0, c);
            draw_circle(cx, y + 10.0 * k, 4.0 * k, c);
        }
    }
    let sq = Rect::new(r.x, r.y, r.w, r.w);
    draw_rectangle_lines(sq.x, sq.y, sq.w, sq.h, 1.0, Color::new(0.55, 0.55, 0.58, 0.7));
    cell_icon(icon, sq);
}

/// Just the icon of an empty cell, filling `sq`.
pub fn cell_icon(icon: CellIcon, sq: Rect) {
    if let Some(t) = win(icon.art()) {
        tex(&t, sq, WHITE);
    } else {
        draw_cell_icon_fallback(icon, sq);
    }
}

/// A coloured frame around a card or cell: green for the unit that acts, blue for a cell to
/// move to or a friendly target, red for a hostile target.
pub fn glow_frame(r: Rect, color: Color, strong: bool) {
    let k = k();
    let w = if strong { 3.0 * k } else { 2.0 * k };
    for i in 0..3 {
        let f = i as f32;
        let a = color.a * [0.95, 0.45, 0.2][i];
        let c = Color { a, ..color };
        draw_rectangle_lines(r.x - f * w * 0.7, r.y - f * w * 0.7, r.w + 2.0 * f * w * 0.7, r.h + 2.0 * f * w * 0.7, w, c);
    }
}

/// A small badge in a card corner (`sign-*`, `army-*`, `Sign-Upgrade`), or a coloured dot.
pub fn badge(name: &str, cx: f32, cy: f32, size: f32, fallback: Color) {
    if let Some(t) = win(name) {
        tex(&t, Rect::new(cx - size / 2.0, cy - size / 2.0, size, size), WHITE);
    } else {
        draw_circle(cx, cy, size * 0.42, Color::new(0.0, 0.0, 0.0, 0.6));
        draw_circle(cx, cy, size * 0.34, fallback);
    }
}

/// A trait icon (`Bonus<N>.lit`, `Bonus-2Row`, `HeroBonus<N>`), or a small shield.
pub fn trait_icon(name: &str, x: f32, y: f32, size: f32) {
    if let Some(t) = win(name) {
        tex(&t, Rect::new(x, y, size, size), WHITE);
    } else {
        let c = Color::new(0.25, 0.35, 0.75, 1.0);
        draw_rectangle(x + size * 0.15, y + size * 0.1, size * 0.7, size * 0.45, c);
        draw_triangle(vec2(x + size * 0.15, y + size * 0.55), vec2(x + size * 0.85, y + size * 0.55), vec2(x + size * 0.5, y + size * 0.95), c);
    }
}

// ------------------------------------------------------------------------------------------
// Animations
// ------------------------------------------------------------------------------------------

/// A spell or hit animation of the original, drawn centred on `c` with `size` width; `t` runs
/// 0..1. False when the install has no such animation (draw a placeholder instead).
pub fn effect(rel: &str, c: Vec2, size: f32, t: f32, color: Color) -> bool {
    let Some(frames) = animation(rel) else { return false };
    let n = frames.len();
    let i = ((t.clamp(0.0, 0.999)) * n as f32) as usize;
    let f = &frames[i.min(n - 1)];
    let h = size * f.height() / f.width();
    tex(f, Rect::new(c.x - size / 2.0, c.y - h / 2.0, size, h), color);
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn img(px: &[[u8; 4]]) -> Image {
        Image { width: px.len() as u32, height: 1, rgba: px.iter().flatten().copied().collect() }
    }

    #[test]
    fn black_keys_out_and_glow_turns_brightness_into_alpha() {
        let mut a = img(&[[0, 0, 0, 255], [200, 100, 50, 255]]);
        transform(&mut a, Fx::KeyBlack);
        assert_eq!(a.pixel(0, 0)[3], 0);
        assert_eq!(a.pixel(1, 0)[3], 255);
        let mut g = img(&[[0, 0, 0, 0], [100, 50, 0, 0]]);
        transform(&mut g, Fx::Glow);
        assert_eq!(g.pixel(0, 0)[3], 0);
        assert_eq!(g.pixel(1, 0), [255, 127, 0, 100]);
    }

    /// Every original picture the interface asks for is in the install and decodes (skipped
    /// without `RAZDOR_DT_DIR`).
    #[test]
    fn the_interface_art_is_in_the_install() {
        let Ok(install) = razdor::dt::install::DtInstall::from_env() else { return };
        let mut names: Vec<String> = [
            "Win-marble", "Win-red", "Win-paper", "Win2a", "SteelLine", "DownCorner", "HintFrame", "Corner-Left", "Corner-Right",
            "Corner_Frame-LU", "Corner_Frame-RU", "Corner_Frame-LD", "Corner_Frame-RD", "CloseButtonRed-Up", "CloseButtonGreen-Up",
            "CloseButtonRed-Down2", "CloseButtonGreen-Down2", "UnitFrame", "Shadow-Empty", "wb1", "wb2", "wb3", "army-2", "army-3",
            "sign-poison", "sign-payment", "Sign-Upgrade", "SI_Helm", "Bonus-2Row", "Bonus-InCastle", "Btn1Up", "Btn2Up", "Btn3Up",
            "Btn1Down", "Btn2Down", "Btn3Down", "smb-up", "smb-down", "smb-disable", "MLBtn1Up", "MLBtn2Up", "MRBtn1Up", "MRBtn2Up",
            "MLBtn1Down", "MLBtn2Down", "MRBtn1Down", "MRBtn2Down", "Res-Magic", "Res-Money", "Res-Income", "Res-Payment",
            "MiniMap_Frame_400x400", "Inventory", "UpgradeTree", "BI_Town", "BI_Castle", "BI_Church", "BI_Ruin", "S_Town", "S_Castle",
            "S_Church", "S_Market", "S_Tavern", "S_Village", "S_Shipyard", "S_Ruin",
        ]
        .iter()
        .map(|n| format!("Windows/{n}.lit"))
        .collect();
        names.extend((1..=8).map(|n| format!("Windows/button-icon-{n}.lit")));
        names.extend((0..=3).flat_map(|n| [format!("Windows/res{n}.lit"), format!("Windows/res{n}-44.lit")]));
        names.extend((1..=6).flat_map(|n| ["TB-", "TBo", "TBd"].map(|s| format!("Windows/{s}{n}_RUS.lit"))));
        names.extend((1..=52).map(|n| format!("Windows/Bonus{n}.lit")));
        names.extend((1..=3).map(|n| format!("Windows/HeroBonus{n}.lit")));
        for a in ["KUSKI", "KUSKIBIG", "CURE"] {
            names.push(format!("Battle/--{a}.ugs"));
        }
        for s in ["S-Light-Front", "S-Fog", "S-Fire", "S-Fontain", "S-Swirl"] {
            names.push(format!("Spells/{s}.ugs"));
        }
        for n in names {
            let frames = install.graphic(&format!("Graphics/{n}")).unwrap_or_else(|e| panic!("{n}: {e}"));
            assert!(!frames.is_empty(), "{n}");
        }
        assert!(ui_text_from(&install.dir, "Army", "Bonus2").is_some_and(|t| !t.is_empty()));
    }

    #[test]
    fn placeholder_marble_tiles() {
        let n = 64;
        let v = marble_noise(n);
        assert_eq!(v.len(), n * n);
        assert!(v.iter().all(|&b| b >= 150));
        assert!(v.iter().any(|&b| b > 200) && v.iter().any(|&b| b < 200), "not flat");
        // Opposite edges meet: the texture repeats without a seam.
        let seam: i32 = (0..n).map(|y| (v[y * n] as i32 - v[y * n + n - 1] as i32).abs()).max().unwrap();
        let inner: i32 = (0..n).map(|y| (v[y * n + 20] as i32 - v[y * n + 21] as i32).abs()).max().unwrap();
        assert!(seam <= inner * 3 + 6, "seam {seam} vs neighbours {inner}");
        assert_eq!(v, marble_noise(n), "deterministic");
    }

    #[test]
    fn grey_and_shade() {
        let mut a = img(&[[255, 0, 0, 200]]);
        transform(&mut a, Fx::Grey);
        let p = a.pixel(0, 0);
        assert!(p[0] == p[1] && p[1] == p[2] && p[3] == 200);
        let mut s = img(&[[255, 255, 255, 255], [0, 0, 0, 255]]);
        transform(&mut s, Fx::Shade);
        assert_eq!(s.pixel(0, 0)[3], 255);
        assert_eq!(s.pixel(1, 0)[3], 0);
    }
}
