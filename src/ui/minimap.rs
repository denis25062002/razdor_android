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
use razdor::rules::map::TileMap;
use razdor::rules::world::LocationKind;

use super::widgets::*;

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


/// The minimap window's side in pixels of the 960×720 video (`MiniMap_Frame_400x400.lit` is
/// 430 px at the original's 1024×768), and its frame's border.
const WINDOW: f32 = 403.0;
const BORDER: f32 = 13.0 * 403.0 / 430.0;

/// Screen rectangle of the minimap picture (inside its frame): a square in the top-right
/// corner of the map view `view`, the whole map in it, one cell per texel (the original's
/// minimap is square for its square maps).
pub fn rect(map: &TileMap, view: Rect) -> Rect {
    rect_at(map, view, super::chrome::k())
}

/// [`rect`] at interface scale `k`.
fn rect_at(map: &TileMap, view: Rect, k: f32) -> Rect {
    let o = outer_at(view, k);
    let b = BORDER * k;
    let inner = Rect::new(o.x + b, o.y + b, o.w - 2.0 * b, o.h - 2.0 * b);
    let (w, h) = (map.w.max(1) as f32, map.h.max(1) as f32);
    let s = (inner.w / w).min(inner.h / h);
    let (pw, ph) = (w * s, h * s);
    Rect::new(inner.x + (inner.w - pw) / 2.0, inner.y + (inner.h - ph) / 2.0, pw, ph)
}

/// Frame included.
pub fn outer(_map: &TileMap, view: Rect) -> Rect {
    outer_at(view, super::chrome::k())
}

fn outer_at(view: Rect, k: f32) -> Rect {
    let side = (WINDOW * k).min(view.w - 4.0).min(view.h - 4.0);
    Rect::new(view.x + view.w - side - 10.0 * k, view.y + 2.0 * k, side, side)
}

/// The original's colours (`Rus_DiscordTimes.ini [Options]`, 0xRRGGBB), else ours.
fn option_color(key: &str, ours: [u8; 3]) -> Color {
    let v = super::chrome::options_value(key).and_then(|v| v.trim().parse::<u32>().ok());
    let [r, g, b] = v.map_or(ours, |v| [(v >> 16) as u8, (v >> 8) as u8, v as u8]);
    Color::from_rgba(r, g, b, 255)
}

/// A minimap symbol of `MM_Icons.ugs`: column and row in its size's grid.
#[derive(Clone, Copy)]
enum Symbol {
    Grid(u32, u32),
    Castle,
}

/// Where a symbol sits in `MM_Icons.ugs` for icons of `size` (0 small 12 px, 1 medium 18,
/// 2 large 24): five rows of two symbols per size, the castles down the left in three sizes.
fn symbol_rect(sym: Symbol, size: usize) -> Rect {
    match sym {
        Symbol::Grid(c, r) => {
            let (x0, cell) = [(0.0, 12.0), (24.0, 18.0), (60.0, 24.0)][size];
            Rect::new(x0 + c as f32 * cell, r as f32 * cell, cell, cell)
        }
        Symbol::Castle => [Rect::new(0.0, 72.0, 26.0, 12.0), Rect::new(0.0, 84.0, 26.0, 20.0), Rect::new(0.0, 104.0, 26.0, 24.0)][size],
    }
}

/// The symbol of a location kind on the minimap, if it has one.
fn symbol(kind: LocationKind) -> Option<Symbol> {
    use LocationKind as K;
    Some(match kind {
        K::Castle | K::Palace | K::Town => Symbol::Castle,
        K::Village => Symbol::Grid(0, 2),
        K::Fort => Symbol::Grid(0, 3),
        K::Ruins => Symbol::Grid(1, 3),
        K::Church => Symbol::Grid(1, 4),
        K::Tavern | K::Market | K::Smithy => Symbol::Grid(0, 4),
        K::Shipyard => Symbol::Grid(1, 1),
        K::Entrance => Symbol::Grid(1, 2),
        K::Altar | K::Obelisk => Symbol::Grid(1, 0),
        K::Camp => Symbol::Grid(0, 1),
        K::StoneBridge | K::WoodenBridge => return None,
    })
}

/// The colour of a location on the minimap, as the original's options give them: villages
/// yellow (orange once their tribute is taken), buildings by owner, ruins grey, harbours blue.
fn location_color(l: &razdor::rules::world::Location) -> Color {
    match l.kind {
        LocationKind::Village if l.tribute_gold <= 0 && l.tribute_mana <= 0 => option_color("ColorVillageEmpty", [255, 160, 0]),
        LocationKind::Village => option_color("ColorVillageFull", [255, 255, 0]),
        LocationKind::Ruins => option_color("ColorRuin", [195, 195, 195]),
        LocationKind::Shipyard => option_color("ColorHarbor", [40, 160, 255]),
        _ => match location_side(l) {
            Side::Player => option_color("ColorBuildingPlayer", [64, 223, 64]),
            Side::Ally => option_color("ColorBuildingAlly", [0, 160, 255]),
            Side::Enemy | Side::Neighbour => option_color("ColorBuildingEnemy", [255, 66, 0]),
            Side::Neutral => option_color("ColorNeutral", [255, 255, 255]),
        },
    }
}

/// Draws the minimap window. `view_world` is the part of the world the map view shows (world
/// units). Returns the world position clicked, if any.
pub fn window(game: &Game, art: Option<&super::dt_art::DtArt>, view: Rect, view_world: Rect, surface_color: fn(u8) -> Color) -> Option<(f32, f32)> {
    let map = &game.world.map;
    let fog = &game.fog;
    let r = rect(map, view);
    let o = outer(map, view);
    // The window: black behind the map, the original's silver frame over it (or a stone-grey
    // one).
    let frame_art = super::chrome::win_fx("MiniMap_Frame_400x400", super::chrome::Fx::KeyBlack);
    draw_rectangle(o.x, o.y, o.w, o.h, BLACK);
    if frame_art.is_none() {
        draw_rectangle_lines(o.x + 1.0, o.y + 1.0, o.w - 2.0, o.h - 2.0, 2.0, Color::new(0.62, 0.64, 0.62, 1.0));
    }
    let tex = cached(&MINI_TEX, fog.fingerprint() ^ (map.w as u64) << 20 ^ map.h as u64 ^ art.is_some() as u64, FilterMode::Linear, || {
        // The ground: the terrain texture's own colours; the map objects on it in their
        // sprites' colours; building footprints (bridges, roads through towns) light.
        let (w, h) = (map.w as usize, map.h as usize);
        let mut rgb = vec![[0u8; 3]; w * h];
        for y in 0..map.h {
            for x in 0..map.w {
                let code = map.surface_code((x, y));
                rgb[y as usize * w + x as usize] = art.and_then(|a| a.minimap_ground(code, x, y)).unwrap_or_else(|| {
                    let [r, g, b, _]: [u8; 4] = surface_color(code).into();
                    [r, g, b]
                });
            }
        }
        // Objects tint the ground a third of the way to their colour: specks of trees,
        // lighter hills, grey mountains (as the original's minimap shows them).
        for o in map.objects_in_rows(0, map.h) {
            let Some(c) = art.and_then(|a| a.minimap_object(o.class, o.sprite)) else { continue };
            for t in razdor::rules::map::object_cells(o) {
                if t.0 >= 0 && t.1 >= 0 && (t.0 as usize) < w && (t.1 as usize) < h {
                    let p = &mut rgb[t.1 as usize * w + t.0 as usize];
                    *p = [0, 1, 2].map(|i| ((2 * p[i] as u32 + c[i] as u32) / 3) as u8);
                }
            }
        }
        // A little darker than the ground's textures (the video: grass (42, 82, 13)
        // against the texture's (53, 94, 11)).
        if art.is_some() {
            rgb.iter_mut().for_each(|p| *p = p.map(|v| (v as u32 * 85 / 100) as u8));
        }
        for l in game.world.locations.iter().filter(|l| l.kind.is_bridge()) {
            for t in l.cells() {
                if t.0 >= 0 && t.1 >= 0 && (t.0 as usize) < w && (t.1 as usize) < h {
                    rgb[t.1 as usize * w + t.0 as usize] = [230, 230, 230];
                }
            }
        }
        // The dark with the map's soft edge.
        let dark = darkness(fog);
        let mut rgba = Vec::with_capacity(w * h * 4);
        for (i, c) in rgb.iter().enumerate() {
            let lit = 255 - dark.get(i).copied().unwrap_or(255) as u32;
            rgba.extend([c[0], c[1], c[2]].map(|v| (v as u32 * lit / 255) as u8));
            rgba.push(255);
        }
        (map.w as u16, map.h as u16, rgba)
    });
    draw_texture_ex(&tex, r.x, r.y, WHITE, DrawTextureParams { dest_size: Some(vec2(r.w, r.h)), ..Default::default() });

    // Cells → minimap pixels (world units: rows are `row_height` apart).
    let rh = map.grid.row_height();
    let k = vec2(r.w / map.w as f32, r.h / map.h as f32);
    let to_mini = |p: (f32, f32)| vec2(r.x + (p.0 + 0.5) * k.x, r.y + (p.1 / rh + 0.5) * k.y);

    // The symbols, in the size that suits the map (small ones for the big maps).
    let icons = super::chrome::win_ugs("MM_Icons");
    let size = if map.w >= 150 { 0 } else if map.w >= 75 { 1 } else { 2 };
    let zoom = super::chrome::k() * 0.9375;
    for l in game.world.locations.iter().filter(|l| !l.kind.is_bridge()) {
        if !l.cells().any(|t| fog.explored(t)) && !fog.explored(l.tile) {
            continue;
        }
        let (ax, ay) = map.center(l.anchor);
        let c = to_mini((ax - (l.size.0 - 1) as f32 / 2.0, ay - (l.size.1 - 1) as f32 * rh / 2.0));
        let col = location_color(l);
        match (&icons, symbol(l.kind)) {
            (Some(t), Some(sym)) => {
                let src = symbol_rect(sym, size);
                let (w, h) = (src.w * zoom, src.h * zoom);
                super::chrome::tex_src(t, src, Rect::new(c.x - w / 2.0, c.y - h / 2.0, w, h), col);
            }
            _ => {
                draw_circle(c.x, c.y, 3.5, Color::new(0.0, 0.0, 0.0, 0.85));
                draw_circle(c.x, c.y, 2.5, col);
            }
        }
    }

    // The hero: a blinking mark in the player's colour.
    let h = to_mini(game.display_pos());
    let pulse = 0.6 + 0.4 * (get_time() as f32 * 5.0).sin().abs();
    let mark = option_color("ColorMarkPlayer", [255, 255, 255]);
    draw_circle(h.x, h.y, 3.5 * zoom.max(1.0), BLACK);
    draw_circle(h.x, h.y, 2.5 * zoom.max(1.0), Color::new(mark.r, mark.g, mark.b, pulse));

    // The view: a light grey box, as the original's.
    let a = to_mini((view_world.x, view_world.y));
    let b = to_mini((view_world.x + view_world.w, view_world.y + view_world.h));
    let (x0, y0) = (a.x.max(r.x), a.y.max(r.y));
    let (x1, y1) = (b.x.min(r.x + r.w), b.y.min(r.y + r.h));
    if x1 > x0 && y1 > y0 {
        draw_rectangle(x0, y0, x1 - x0, y1 - y0, Color::new(1.0, 1.0, 1.0, 0.18));
        draw_rectangle_lines(x0, y0, x1 - x0, y1 - y0, 1.0, Color::new(0.85, 0.85, 0.85, 0.6));
    }

    if let Some(t) = frame_art {
        super::chrome::tex(&t, o, WHITE);
    }

    let m = Vec2::from(crate::ui::widgets::pointer());
    (clicked() && r.contains(m)).then(|| ((m.x - r.x) / k.x - 0.5, ((m.y - r.y) / k.y - 0.5) * rh))
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
    fn minimap_is_square_for_square_maps_and_keeps_other_aspects() {
        let codes = vec![0u8; 200 * 100];
        let map = TileMap::from_codes(Grid::Square8, 200, 100, &codes, vec![]);
        let view = Rect::new(0.0, 0.0, 1000.0, 700.0);
        let r = rect_at(&map, view, 1.0);
        assert!((r.w / r.h - 2.0).abs() < 1e-3, "one cell per texel: {r:?}");
        let o = outer_at(view, 1.0);
        assert!(o.contains(r.point()) && r.x + r.w <= o.x + o.w && o.x + o.w <= 1000.0 && o.y >= 0.0);
        assert!((o.w - o.h).abs() < 1e-3, "the frame is square");
        let square = TileMap::from_codes(Grid::Square8, 50, 50, &vec![0u8; 2500], vec![]);
        let r = rect_at(&square, view, 1.0);
        assert!((r.w - r.h).abs() < 1e-3);
    }

    #[test]
    fn minimap_symbols_lie_inside_the_atlas() {
        for size in 0..3 {
            for sym in [Symbol::Castle, Symbol::Grid(0, 0), Symbol::Grid(1, 4)] {
                let r = symbol_rect(sym, size);
                assert!(r.x >= 0.0 && r.y >= 0.0 && r.x + r.w <= 108.0 && r.y + r.h <= 128.0, "{size} {r:?}");
            }
        }
    }
}
