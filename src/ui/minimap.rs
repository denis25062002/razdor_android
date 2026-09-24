//! The fog layer over the world map and the minimap window (`rules::fog`,
//! `docs/reference/video-notes.md` §1).
//!
//! - Fog: unexplored cells are black. The layer is a texture with one texel per cell, drawn
//!   scaled over the map with linear filtering; explored cells next to the dark are shaded by
//!   how much dark lies around them, so the edge is a soft feathered band like the original's.
//! - Minimap: a toggle window in the top-right corner of the map view (bottom-bar "Map" button
//!   or M). The whole map scaled down, explored cells in their terrain colour and the rest
//!   black, locations as small icons in their owner's colour, the hero, and a light rectangle
//!   for the view. A click on it moves the camera there (as in the video); walking still
//!   needs a click on the map.
//!
//! Both textures are rebuilt only when the explored set changes.

use std::cell::RefCell;

use macroquad::prelude::*;

use razdor::rules::fog::{location_side, Fog, Side};
use razdor::rules::game::Game;
use razdor::rules::map::{surface_minutes, TileMap};
use razdor::rules::world::LocationKind;

use super::widgets::*;

/// Largest side of the minimap picture, in screen pixels.
const MINIMAP_MAX: f32 = 360.0;
const FRAME: f32 = 8.0;
/// Explored cells within this many cells of the dark are shaded (the feathered edge).
const FEATHER: i32 = 2;

struct Cached {
    key: u64,
    tex: Texture2D,
}

thread_local! {
    static FOG_TEX: RefCell<Option<Cached>> = const { RefCell::new(None) };
    static MINI_TEX: RefCell<Option<Cached>> = const { RefCell::new(None) };
}

/// A texture from `cache`, rebuilt by `make` (w, h, rgba) when `key` changes.
fn cached(cache: &'static std::thread::LocalKey<RefCell<Option<Cached>>>, key: u64, filter: FilterMode, make: impl FnOnce() -> (u16, u16, Vec<u8>)) -> Texture2D {
    cache.with(|c| {
        let mut c = c.borrow_mut();
        if c.as_ref().is_none_or(|c| c.key != key) {
            let (w, h, rgba) = make();
            let tex = Texture2D::from_rgba8(w, h, &rgba);
            tex.set_filter(filter);
            *c = Some(Cached { key, tex });
        }
        c.as_ref().unwrap().tex.clone()
    })
}

/// Darkness (0 lit … 255 black) of every cell: the share of unexplored cells within
/// [`FEATHER`] cells (a box blur through a summed-area table), eased by a smoothstep. The
/// 0.5 contour runs along the border of the explored ground with its corners rounded, so
/// explored cells at the edge are shaded and dark cells at the edge let a little through:
/// the soft edge of the original. Deep in the dark it is black, well inside it is clear.
pub fn darkness(fog: &Fog) -> Vec<u8> {
    let (w, h) = (fog.w.max(0) as usize, fog.h.max(0) as usize);
    let mut sum = vec![0u32; (w + 1) * (h + 1)];
    for y in 0..h {
        let mut row = 0;
        for x in 0..w {
            row += (!fog.explored((x as i32, y as i32))) as u32;
            sum[(y + 1) * (w + 1) + x + 1] = sum[y * (w + 1) + x + 1] + row;
        }
    }
    let f = FEATHER as usize;
    let mut out = vec![0u8; w * h];
    for y in 0..h {
        for x in 0..w {
            // Outside the map counts as neither: the window shrinks at the map's edges.
            let (x0, x1, y0, y1) = (x.saturating_sub(f), (x + f + 1).min(w), y.saturating_sub(f), (y + f + 1).min(h));
            let dark = sum[y1 * (w + 1) + x1] + sum[y0 * (w + 1) + x0] - sum[y0 * (w + 1) + x1] - sum[y1 * (w + 1) + x0];
            let share = dark as f32 / ((x1 - x0) * (y1 - y0)) as f32;
            let t = ((share - 0.2) / 0.5).clamp(0.0, 1.0);
            out[y * w + x] = (t * t * (3.0 - 2.0 * t) * 255.0).round() as u8;
        }
    }
    out
}

/// Draws the fog over the map. `tl` and `br` are the screen positions of world points
/// `(-0.5, -0.5·row)` and `(w − 0.5, (h − 0.5)·row)`: the outer corners of the edge cells,
/// so texel centres fall on cell centres.
pub fn draw_fog(fog: &Fog, tl: Vec2, br: Vec2) {
    if !fog.enabled || fog.w <= 0 || fog.h <= 0 {
        return;
    }
    let tex = cached(&FOG_TEX, fog.fingerprint(), FilterMode::Linear, || {
        let rgba = darkness(fog).into_iter().flat_map(|a| [0, 0, 0, a]).collect();
        (fog.w as u16, fog.h as u16, rgba)
    });
    draw_texture_ex(&tex, tl.x, tl.y, WHITE, DrawTextureParams { dest_size: Some(br - tl), ..Default::default() });
}

/// Minimap colour of a side (see [`Side::rgb`]).
pub fn side_color(side: Side) -> Color {
    let [r, g, b] = side.rgb();
    Color::from_rgba(r, g, b, 255)
}

/// Screen rectangle of the minimap picture (inside its frame), in the top-right corner of
/// the map view `view`.
pub fn rect(map: &TileMap, view: Rect) -> Rect {
    let (w, h) = (map.w.max(1) as f32, map.h.max(1) as f32 * map.grid.row_height());
    let k = MINIMAP_MAX / w.max(h);
    let (pw, ph) = (w * k, h * k);
    Rect::new(view.x + view.w - pw - FRAME - 10.0, view.y + FRAME + 10.0, pw, ph)
}

/// Frame included.
pub fn outer(map: &TileMap, view: Rect) -> Rect {
    let r = rect(map, view);
    Rect::new(r.x - FRAME, r.y - FRAME, r.w + 2.0 * FRAME, r.h + 2.0 * FRAME)
}

/// Draws the minimap window. `view_world` is the part of the world the map view shows (world
/// units). Returns the world position clicked, if any.
pub fn window(game: &Game, view: Rect, view_world: Rect, surface_color: fn(u8) -> Color) -> Option<(f32, f32)> {
    let map = &game.world.map;
    let fog = &game.fog;
    let r = rect(map, view);
    let o = outer(map, view);
    // Stone-grey frame.
    draw_rectangle(o.x, o.y, o.w, o.h, Color::new(0.30, 0.31, 0.30, 1.0));
    draw_rectangle_lines(o.x + 1.0, o.y + 1.0, o.w - 2.0, o.h - 2.0, 2.0, Color::new(0.62, 0.64, 0.62, 1.0));
    draw_rectangle_lines(r.x - 2.0, r.y - 2.0, r.w + 4.0, r.h + 4.0, 2.0, Color::new(0.12, 0.12, 0.12, 1.0));
    let tex = cached(&MINI_TEX, fog.fingerprint() ^ (map.w as u64) << 20 ^ map.h as u64, FilterMode::Linear, || {
        let mut rgba = Vec::with_capacity((map.w * map.h * 4) as usize);
        for y in 0..map.h {
            for x in 0..map.w {
                if !fog.explored((x, y)) {
                    rgba.extend([0, 0, 0, 255]);
                    continue;
                }
                let mut c = surface_color(map.surface_code((x, y)));
                if !map.passable((x, y)) && surface_minutes(map.surface((x, y))).is_some() {
                    // Mountains, rocks and thickets: darker, so the passes show.
                    c = Color::new(c.r * 0.62, c.g * 0.62, c.b * 0.62, 1.0);
                }
                let [r, g, b, _] = c.into();
                rgba.extend([r, g, b, 255]);
            }
        }
        (map.w as u16, map.h as u16, rgba)
    });
    draw_texture_ex(&tex, r.x, r.y, WHITE, DrawTextureParams { dest_size: Some(vec2(r.w, r.h)), ..Default::default() });

    // World units → minimap pixels.
    let world_h = map.h as f32 * map.grid.row_height();
    let k = vec2(r.w / map.w as f32, r.h / world_h);
    let rh = map.grid.row_height();
    let to_mini = |p: (f32, f32)| vec2(r.x + (p.0 + 0.5) * k.x, r.y + (p.1 + 0.5 * rh) * k.y);

    for l in game.world.locations.iter().filter(|l| !l.kind.is_bridge()) {
        if !l.cells().any(|t| fog.explored(t)) && !fog.explored(l.tile) {
            continue;
        }
        let (ax, ay) = map.center(l.anchor);
        let c = to_mini((ax - (l.size.0 - 1) as f32 / 2.0, ay - (l.size.1 - 1) as f32 * rh / 2.0));
        let col = side_color(location_side(l));
        let dark = Color::new(0.0, 0.0, 0.0, 0.85);
        match l.kind {
            LocationKind::Castle | LocationKind::Palace | LocationKind::Fort | LocationKind::Town => {
                let s = if l.kind == LocationKind::Fort { 7.0 } else { 9.0 };
                draw_rectangle(c.x - s / 2.0 - 1.0, c.y - s / 2.0 - 1.0, s + 2.0, s + 2.0, dark);
                draw_rectangle(c.x - s / 2.0, c.y - s / 2.0, s, s, col);
                // Battlements.
                draw_rectangle(c.x - s / 2.0, c.y - s / 2.0 - 2.0, 2.0, 2.0, col);
                draw_rectangle(c.x + s / 2.0 - 2.0, c.y - s / 2.0 - 2.0, 2.0, 2.0, col);
            }
            LocationKind::Village => {
                draw_triangle(vec2(c.x, c.y - 5.0), vec2(c.x - 4.5, c.y - 0.5), vec2(c.x + 4.5, c.y - 0.5), dark);
                draw_rectangle(c.x - 4.0, c.y - 1.0, 8.0, 5.0, dark);
                draw_triangle(vec2(c.x, c.y - 4.0), vec2(c.x - 3.5, c.y - 0.5), vec2(c.x + 3.5, c.y - 0.5), col);
                draw_rectangle(c.x - 3.0, c.y - 0.5, 6.0, 3.5, col);
            }
            _ => {
                draw_circle(c.x, c.y, 3.5, dark);
                draw_circle(c.x, c.y, 2.5, col);
            }
        }
    }

    // The hero: a blinking white marker.
    let h = to_mini(game.pos);
    let pulse = 0.6 + 0.4 * (get_time() as f32 * 5.0).sin().abs();
    draw_circle(h.x, h.y, 4.5, BLACK);
    draw_circle(h.x, h.y, 3.5, Color::new(1.0, 1.0, 1.0, pulse));

    // The view: a light rectangle, clipped to the picture.
    let a = to_mini((view_world.x, view_world.y));
    let b = to_mini((view_world.x + view_world.w, view_world.y + view_world.h));
    let (x0, y0) = (a.x.max(r.x), a.y.max(r.y));
    let (x1, y1) = (b.x.min(r.x + r.w), b.y.min(r.y + r.h));
    if x1 > x0 && y1 > y0 {
        draw_rectangle(x0, y0, x1 - x0, y1 - y0, Color::new(1.0, 1.0, 1.0, 0.14));
        draw_rectangle_lines(x0, y0, x1 - x0, y1 - y0, 1.5, Color::new(1.0, 1.0, 0.9, 0.8));
    }

    let m = Vec2::from(mouse_position());
    (clicked() && r.contains(m)).then(|| ((m.x - r.x) / k.x - 0.5, (m.y - r.y) / k.y - 0.5 * rh))
}

/// The bottom-bar toggle, lit while the minimap is open.
pub fn toggle_button(x: f32, y: f32, open: bool) -> bool {
    let pressed = button(x, y, 120.0, 40.0, "Map (M)", true);
    if open {
        draw_rectangle_lines(x - 2.0, y - 2.0, 124.0, 44.0, 2.0, Color::new(1.0, 0.6, 0.2, 1.0));
    }
    pressed
}

#[cfg(test)]
mod tests {
    use super::*;
    use razdor::rules::map::Grid;

    #[test]
    fn darkness_is_black_in_the_dark_and_feathered_at_the_edge() {
        // Explored: the left half.
        let (w, h) = (40, 20);
        let mut fog = Fog::new(w, h);
        for y in 0..h {
            for x in 0..20 {
                fog.mark((x, y));
            }
        }
        let d = darkness(&fog);
        let at = |x: i32, y: i32| d[(y * w + x) as usize];
        assert_eq!(at(39, 10), 255, "deep in the dark");
        assert_eq!(at(5, 10), 0, "well inside is clear");
        assert!(at(19, 10) > 0 && at(19, 10) < 128, "explored edge cell shaded: {}", at(19, 10));
        assert!(at(20, 10) >= 128 && at(20, 10) < 255, "dark edge cell mostly dark: {}", at(20, 10));
        assert!(at(17, 10) <= at(19, 10) && at(20, 10) <= at(22, 10), "darker outwards");
        assert!(darkness(&Fog::disabled(3, 3)).iter().all(|&a| a == 0));
    }

    #[test]
    fn side_colours_follow_the_editor() {
        assert_eq!(side_color(Side::Player), Color::from_rgba(70, 200, 90, 255));
        assert_eq!(side_color(Side::Enemy), Color::from_rgba(220, 60, 50, 255));
        assert_eq!(side_color(Side::Ally), Color::from_rgba(70, 130, 230, 255));
    }

    #[test]
    fn minimap_keeps_the_map_aspect() {
        let codes = vec![0u8; 200 * 100];
        let map = TileMap::from_codes(Grid::Square8, 200, 100, &codes, vec![]);
        let r = rect(&map, Rect::new(0.0, 0.0, 1000.0, 700.0));
        assert!((r.w - MINIMAP_MAX).abs() < 1e-3);
        assert!((r.h / r.w - 100.0 * Grid::Square8.row_height() / 200.0).abs() < 1e-4);
        assert!(r.x + r.w < 1000.0 && r.y > 0.0);
    }
}
