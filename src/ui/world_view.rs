//! The world map: terrain, objects, buildings, armies, the party, its clock and money.
//!
//! With a Discord Times install the original terrain textures, map objects, buildings and
//! map figures are drawn (decoded at runtime by [`DtArt`]); otherwise coloured cells and
//! simple shapes. Only the visible cells are drawn, so 200×200 maps stay fast.

use std::collections::VecDeque;

use macroquad::prelude::*;

use razdor::i18n::{n_, tr};
use razdor::rules::battle::Team;
use razdor::trf;
use razdor::rules::clock::duration_label;
use razdor::rules::content::HeroClass;
use razdor::rules::game::{Event, Foe, Game};
use razdor::rules::town::first_tab;
use razdor::rules::map::{object_class, Decoration, Grid, Tile, TileMap};
use razdor::rules::world::{Army, Location, LocationKind, Troop};

use super::assets::Assets;
use super::audio::{cue, Cue};
use super::building_view::BuildingView;
use super::dialog::Dialog;
use super::dt_art::DtArt;
use super::game_bar::{self, BarButton, Look};
use super::minimap;
use super::saves::{self, Back, LoadView, SaveView};
use super::story;
use super::widgets::*;
use super::Screen;

/// Screen pixels per world unit (one cell width) at zoom 1: the original's 32 px cells.
const PX: f32 = 32.0;
/// Sail colour of the hero's own ship.
const HERO_SAIL: Color = Color::new(0.35, 0.8, 0.45, 1.0);
fn bar_h() -> f32 {
    super::chrome::bar_height()
}
use super::dialog::MANA;

/// World-map view state kept between frames.
pub struct MapView {
    pub zoom: f32,
    /// The minimap window is open.
    pub minimap: bool,
    /// Where the camera looks when moved by the minimap (world units); `None` follows the hero.
    pub look: Option<(f32, f32)>,
    /// Places the scenario's events have shown (lanterns, shown armies), first in line: the
    /// camera flies to each in turn and its uncovered cells fade in from the fog.
    shows: VecDeque<Showing>,
}

impl Default for MapView {
    fn default() -> Self {
        MapView { zoom: 1.0, minimap: false, look: None, shows: VecDeque::new() }
    }
}

/// Seconds the camera takes to reach a shown place, then the uncovered area takes to fade
/// in, then the view rests there before the next place.
const SHOW_PAN: f64 = 0.8;
const SHOW_FADE: f64 = 1.2;
const SHOW_REST: f64 = 0.4;

/// A place an event showed: the cells it uncovered stay dark until the camera is there
/// (after the event's message is read), then fade in.
struct Showing {
    /// World position of the place.
    at: (f32, f32),
    /// The uncovered cells, as a fog-sized mask (alpha 255 on them).
    mask: Option<Texture2D>,
    /// When the camera set off, and from where.
    started: Option<(f64, (f32, f32))>,
}

impl Showing {
    fn new(game: &Game, shown: &razdor::rules::game::Shown) -> Showing {
        let fog = &game.fog;
        let mask = (!shown.cells.is_empty() && fog.w > 0 && fog.h > 0).then(|| {
            let mut rgba = vec![0u8; (fog.w * fog.h * 4) as usize];
            for &(x, y) in &shown.cells {
                rgba[((y * fog.w + x) * 4 + 3) as usize] = 255;
            }
            let t = Texture2D::from_rgba8(fog.w as u16, fog.h as u16, &rgba);
            t.set_filter(FilterMode::Linear);
            t
        });
        Showing { at: game.world.map.center(shown.at), mask, started: None }
    }

    /// How dark the uncovered cells still are (1 until the camera arrives).
    fn darkness(&self, now: f64) -> f32 {
        match self.started {
            Some((t0, _)) => (1.0 - ((now - t0 - SHOW_PAN) / SHOW_FADE).clamp(0.0, 1.0)) as f32,
            None => 1.0,
        }
    }
}

impl MapView {
    pub fn reset(&mut self) {
        self.look = None;
    }

    /// A game was loaded: the places the old one was about to show are dropped.
    pub fn forget_shows(&mut self) {
        self.shows.clear();
    }
}

fn rgb(r: u8, g: u8, b: u8) -> Color {
    Color::from_rgba(r, g, b, 255)
}

/// Placeholder colour of a terrain code (our own palette).
pub(super) fn surface_color(code: u8) -> Color {
    match code {
        0 => rgb(120, 170, 200),
        1 => rgb(60, 110, 180),
        2 => rgb(28, 60, 130),
        3 => rgb(190, 70, 30),
        4 => rgb(176, 150, 104),
        5 => rgb(96, 146, 64),
        6 => rgb(112, 160, 70),
        7 => rgb(176, 170, 100),
        8 => rgb(86, 112, 80),
        9 => rgb(52, 70, 52),
        10 => rgb(222, 204, 140),
        11 => rgb(160, 116, 80),
        12 => rgb(140, 132, 122),
        13 => rgb(92, 66, 54),
        14 => rgb(230, 234, 240),
        _ => rgb(250, 250, 255),
    }
}

/// Faction colour (player green, ally blue, neighbour yellow, enemy red, as the editor).
pub(super) fn faction_color(faction: u8) -> Color {
    match faction {
        1 => rgb(70, 200, 90),
        2 => rgb(70, 130, 230),
        3 => rgb(230, 200, 60),
        4 => rgb(220, 60, 50),
        _ => rgb(200, 200, 200),
    }
}

struct Camera {
    /// World pixel (at this zoom) at the top-left of the map view.
    origin: Vec2,
    view: Rect,
    /// Screen pixels per world unit.
    scale: f32,
    grid: Grid,
}

impl Camera {
    /// Centred on world position `at` (clamped to the map), in the map view left of the
    /// side panel.
    fn looking_at(game: &Game, zoom: f32, at: (f32, f32)) -> Camera {
        Camera::looking_in(game, zoom, at, Rect::new(0.0, 0.0, screen_width(), screen_height() - bar_h()))
    }

    fn looking_in(game: &Game, zoom: f32, at: (f32, f32), view: Rect) -> Camera {
        let map = &game.world.map;
        let scale = PX * zoom;
        let rh = map.grid.row_height();
        let world = vec2((map.w as f32 + 0.5) * scale, (map.h as f32) * rh * scale);
        let pad = vec2(0.5 * scale, 0.5 * rh * scale);
        let centre = Vec2::from(at) * scale + pad;
        let mut origin = centre - vec2(view.w, view.h) / 2.0;
        origin.x = origin.x.clamp(0.0, (world.x - view.w).max(0.0));
        origin.y = origin.y.clamp(0.0, (world.y - view.h).max(0.0));
        Camera { origin: origin - pad, view, scale, grid: map.grid }
    }

    /// Screen position of a world-space point.
    fn to_screen(&self, p: (f32, f32)) -> Vec2 {
        Vec2::from(p) * self.scale - self.origin + vec2(self.view.x, self.view.y)
    }

    fn cell_centre(&self, t: Tile) -> Vec2 {
        self.to_screen(self.grid.center(t))
    }

    /// Cell size on screen.
    fn cell_size(&self) -> Vec2 {
        vec2(self.scale, self.scale * self.grid.row_height())
    }

    /// The part of the world in view, in world units.
    fn world_rect(&self) -> Rect {
        let o = self.origin / self.scale;
        Rect::new(o.x, o.y, self.view.w / self.scale, self.view.h / self.scale)
    }

    /// Screen corners of the fog grid over the whole map.
    fn fog_corners(&self, game: &Game) -> (Vec2, Vec2) {
        let map = &game.world.map;
        let rh = map.grid.row_height();
        (self.to_screen((-0.5, -0.5 * rh)), self.to_screen((map.w as f32 - 0.5, (map.h as f32 - 0.5) * rh)))
    }

    /// The fog layer over the whole map (`ui::minimap`).
    fn draw_fog(&self, game: &Game) {
        let (tl, br) = self.fog_corners(game);
        minimap::draw_fog(&game.fog, tl, br);
    }

    /// The fog still over places being shown, as dark as each one's fade has left it.
    fn draw_showing(&self, game: &Game, shows: &VecDeque<Showing>, now: f64) {
        let (tl, br) = self.fog_corners(game);
        for s in shows {
            if let Some(mask) = &s.mask {
                let a = s.darkness(now);
                if a > 0.0 {
                    draw_texture_ex(mask, tl.x, tl.y, Color::new(0.0, 0.0, 0.0, a), DrawTextureParams { dest_size: Some(br - tl), ..Default::default() });
                }
            }
        }
    }

    fn tile_under_mouse(&self) -> Option<Tile> {
        let m = Vec2::from(crate::ui::widgets::pointer());
        if !self.view.contains(m) {
            return None;
        }
        let w = (m - vec2(self.view.x, self.view.y) + self.origin) / self.scale;
        Some(self.grid.tile_at((w.x, w.y)))
    }

    /// Visible cell ranges (cols, rows), half-open, clamped to the map.
    fn visible(&self, map: &TileMap) -> ((i32, i32), (i32, i32)) {
        let top_left = self.origin / self.scale;
        let rh = self.grid.row_height();
        let c0 = (top_left.x - 1.0).floor() as i32;
        let r0 = (top_left.y / rh - 1.0).floor() as i32;
        let c1 = c0 + (self.view.w / self.scale) as i32 + 3;
        let r1 = r0 + (self.view.h / (self.scale * rh)) as i32 + 3;
        ((c0.max(0), c1.min(map.w)), (r0.max(0), r1.min(map.h)))
    }
}

/// Draws `tex` into `dest`, sampling it from world pixel `src` (unscaled) with wrap-around,
/// so neighbouring cells continue the texture seamlessly.
pub(super) fn draw_wrapped(tex: &Texture2D, dest: Rect, src: Vec2, src_size: Vec2) {
    let (tw, th) = (tex.width(), tex.height());
    let u0 = src.x.rem_euclid(tw);
    let v0 = src.y.rem_euclid(th);
    let k = vec2(dest.w / src_size.x, dest.h / src_size.y);
    let mut v = v0;
    let mut dy = 0.0;
    while dy < src_size.y - 0.01 {
        let hgt = (th - v).min(src_size.y - dy);
        let mut u = u0;
        let mut dx = 0.0;
        while dx < src_size.x - 0.01 {
            let wid = (tw - u).min(src_size.x - dx);
            draw_texture_ex(
                tex,
                dest.x + dx * k.x,
                dest.y + dy * k.y,
                WHITE,
                DrawTextureParams {
                    dest_size: Some(vec2(wid * k.x + 0.6, hgt * k.y + 0.6)),
                    source: Some(Rect::new(u, v, wid, hgt)),
                    ..Default::default()
                },
            );
            dx += wid;
            u = 0.0;
        }
        dy += hgt;
        v = 0.0;
    }
}

fn draw_terrain(game: &Game, art: Option<&DtArt>, cam: &Camera) {
    let map = &game.world.map;
    if let Some(layer) = art.and_then(|a| a.terrain_layer()).filter(|_| cam.grid == Grid::Square8) {
        // The map area in view, in world units (cells span ±½ around their centres).
        let rh = cam.grid.row_height();
        let (map_tl, map_br) = (vec2(-0.5, -0.5 * rh), vec2(map.w as f32 - 0.5, (map.h as f32 - 0.5) * rh));
        let w = cam.world_rect();
        let (tl, br) = (map_tl.max(w.point()), map_br.min(w.point() + w.size()));
        if tl.x < br.x && tl.y < br.y {
            let (a, b) = (cam.to_screen(tl.into()), cam.to_screen(br.into()));
            let view = vec4(tl.x, tl.y / rh, br.x, br.y / rh);
            layer.draw(map.surface_codes(), (map.w as u32, map.h as u32), Rect::new(a.x, a.y, b.x - a.x, b.y - a.y), view, vec2(PX, PX * rh));
        }
        return;
    }
    let ((c0, c1), (r0, r1)) = cam.visible(map);
    let size = cam.cell_size();
    let unscaled = vec2(PX, PX * cam.grid.row_height());
    // One pass per terrain code keeps texture switches (and draw calls) few.
    let mut present = [false; 16];
    for y in r0..r1 {
        for x in c0..c1 {
            present[(map.surface_code((x, y)) & 15) as usize] = true;
        }
    }
    for code in (0..16u8).filter(|c| present[*c as usize]) {
        let tex = art.and_then(|a| a.terrain(code));
        for y in r0..r1 {
            for x in c0..c1 {
                if map.surface_code((x, y)) != code {
                    continue;
                }
                let c = cam.cell_centre((x, y));
                let dest = Rect::new(c.x - size.x / 2.0, c.y - size.y / 2.0, size.x, size.y);
                match &tex {
                    Some(tex) => {
                        // Texture space is world space: cell (x, y) samples at its own pixels.
                        let (wx, wy) = cam.grid.center((x, y));
                        draw_wrapped(tex, dest, vec2(wx * PX, wy * PX), unscaled);
                    }
                    None => draw_rectangle(dest.x, dest.y, dest.w + 0.5, dest.h + 0.5, surface_color(code)),
                }
            }
        }
    }
}

/// Something drawn in painter's order (by the bottom edge on screen).
enum Drawable {
    Object(Decoration),
    Building(usize),
    Army(usize),
    /// The hero's ship, waiting where he left it.
    Ship,
    Hero,
}

/// Bottom-centre of a location's footprint, in world units.
fn footprint_base(grid: Grid, l: &Location) -> (f32, f32) {
    let rh = grid.row_height();
    let (x, y) = grid.center(l.anchor);
    (x - (l.size.0 - 1) as f32 / 2.0, y + rh / 2.0)
}

fn draw_object(o: &Decoration, art: Option<&DtArt>, cam: &Camera) {
    let c = cam.cell_centre(o.tile);
    let base = vec2(c.x, c.y + cam.cell_size().y / 2.0);
    let zoom = cam.scale / PX;
    if let Some((atlas, r)) = art.and_then(|a| a.map_atlas()).and_then(|at| Some((at, at.decoration(o.class, o.sprite)?))) {
        let (w, h) = (r.w * zoom, r.h * zoom);
        draw_texture_ex(&atlas.texture, base.x - w / 2.0, base.y - h, WHITE, DrawTextureParams { dest_size: Some(vec2(w, h)), source: Some(r), ..Default::default() });
        return;
    }
    let s = cam.scale;
    use object_class::*;
    match o.class {
        TREES | DEAD_TREES => {
            let col = if o.class == TREES { rgb(40, 100, 44) } else { rgb(110, 90, 50) };
            draw_triangle(vec2(base.x, base.y - s * 1.1), vec2(base.x - s * 0.35, base.y - s * 0.1), vec2(base.x + s * 0.35, base.y - s * 0.1), col);
            draw_line(base.x, base.y, base.x, base.y - s * 0.15, 2.0, rgb(80, 60, 40));
        }
        THICKET => draw_circle(base.x, base.y - s * 0.4, s * 0.42, rgb(24, 64, 30)),
        MOUNTAINS | DARK_MOUNTAINS => {
            let k = 1.0 + (o.sprite / 10) as f32 * 0.5;
            let top = vec2(base.x, base.y - s * 0.9 * k);
            draw_triangle(top, vec2(base.x - s * 0.6 * k, base.y), vec2(base.x + s * 0.6 * k, base.y), rgb(120, 116, 112));
            draw_triangle(top, vec2(top.x - s * 0.15 * k, top.y + s * 0.2 * k), vec2(top.x + s * 0.15 * k, top.y + s * 0.2 * k), rgb(235, 235, 240));
        }
        ROCKS => draw_circle(base.x, base.y - s * 0.2, s * 0.25, rgb(150, 150, 150)),
        _ => {
            let k = 1.0 + (o.sprite / 10) as f32 * 0.4;
            draw_ellipse(base.x, base.y - s * 0.2 * k, s * 0.55 * k, s * 0.25 * k, 0.0, rgb(120, 140, 70));
        }
    }
}

fn draw_building(l: &Location, art: Option<&DtArt>, cam: &Camera) {
    let base = cam.to_screen(footprint_base(cam.grid, l));
    let zoom = cam.scale / PX;
    let sprite = art.and_then(|a| a.map_atlas()).and_then(|at| Some((at, at.building(l.picture.0, l.picture.1)?)));
    let (w, h) = if let Some((atlas, r)) = sprite {
        let (w, h) = (r.w * zoom, r.h * zoom);
        draw_texture_ex(&atlas.texture, base.x - w / 2.0, base.y - h, WHITE, DrawTextureParams { dest_size: Some(vec2(w, h)), source: Some(r), ..Default::default() });
        (w, h)
    } else {
        let (w, h) = (l.size.0 as f32 * cam.scale, (l.size.1 as f32 * cam.cell_size().y).max(cam.scale * 0.8));
        let wall = match l.kind {
            LocationKind::Castle | LocationKind::Fort | LocationKind::Palace => rgb(196, 192, 184),
            LocationKind::Ruins | LocationKind::Camp => rgb(110, 100, 90),
            LocationKind::StoneBridge => rgb(150, 145, 140),
            LocationKind::WoodenBridge => rgb(140, 100, 60),
            LocationKind::Church | LocationKind::Altar | LocationKind::Obelisk => rgb(236, 232, 224),
            _ => rgb(206, 180, 140),
        };
        draw_rectangle(base.x - w / 2.0, base.y - h, w, h, wall);
        draw_rectangle_lines(base.x - w / 2.0, base.y - h, w, h, 1.5, rgb(70, 60, 50));
        if !l.kind.is_bridge() {
            let roof = if l.kind == LocationKind::Camp && l.cleared { rgb(60, 56, 50) } else { rgb(170, 64, 48) };
            draw_triangle(vec2(base.x, base.y - h - h * 0.5), vec2(base.x - w / 2.0, base.y - h), vec2(base.x + w / 2.0, base.y - h), roof);
            text_centered(&l.kind.label()[..1], base.x, base.y - h * 0.3, (h * 0.6).clamp(10.0, 30.0), BLACK);
        }
        (w, h)
    };
    // A pennant in the owner's colour over castles, forts, towns and villages.
    if matches!(l.kind, LocationKind::Castle | LocationKind::Fort | LocationKind::Town | LocationKind::Village) {
        let (px, py) = (base.x - w * 0.3, base.y - h * 0.9);
        let col = if l.owned() { faction_color(1) } else { faction_color(l.faction) };
        draw_line(px, py, px, py + 16.0 * zoom, 2.0, BLACK);
        draw_triangle(vec2(px, py), vec2(px + 12.0 * zoom, py + 4.0 * zoom), vec2(px, py + 8.0 * zoom), col);
    }
}

/// `Graphics/Units/*.ugs` figure for an army's map model or the hero's class.
pub(super) fn figure_stem(model: u8) -> &'static str {
    match model {
        1 => "Hero-Knight",
        2 => "Hero-Mage",
        3 => "Hero-Ranger",
        5 => "Rogue",
        6 => "Peasant",
        10 => "Necromant",
        11 => "Ghost",
        12 => "Zombie",
        _ => "Knight",
    }
}

/// Sheet row for a heading (screen dx, dy): rows run clockwise from north-west.
fn facing_row(d: Vec2) -> f32 {
    if d.length_squared() < 1e-6 {
        return 5.0; // facing the viewer
    }
    let dir = ((d.x.atan2(-d.y) / std::f32::consts::FRAC_PI_4).round() as i32).rem_euclid(8);
    ((dir + 1) % 8) as f32
}

/// The original's ship sprites (`Graphics/Units`) by ship type (army byte 72,
/// `rules::ships::kind`): the hero's galley, pirates and merchants.
fn ship_stem(kind: u8) -> &'static str {
    match kind {
        razdor::rules::ships::kind::PIRATE => "Ship-Pirat",
        razdor::rules::ships::kind::MERCHANT => "Ship-Merchant",
        _ => "Hero-Ship-Vesla",
    }
}

/// How a sprite stands on its point: a figure's feet near the frame's bottom, with a
/// shadow; a ship's waterline across the frame's middle (its reflection is in the art).
#[derive(Clone, Copy)]
enum Stand {
    Feet,
    Afloat,
}

/// Draws a map figure (or ship) at world `pos`, heading towards `next`, drawn at one
/// sprite pixel per map pixel whatever its frame size. Returns false if the install has no
/// such sprite.
fn draw_figure(art: Option<&DtArt>, stem: &str, pos: (f32, f32), next: Option<(f32, f32)>, cam: &Camera, stand: Stand) -> bool {
    let Some(sheet) = art.and_then(|a| a.figure_sheet(stem)) else { return false };
    let n = sheet.width() / 8.0;
    let p = cam.to_screen(pos);
    let heading = next.map_or(Vec2::ZERO, |n| cam.to_screen(n) - p);
    let row = facing_row(heading);
    let frame = if next.is_some() { ((get_time() * 10.0) as i32 % 8) as f32 } else { 0.0 };
    let size = n * cam.scale / PX;
    let dest = match stand {
        Stand::Feet => {
            draw_ellipse(p.x, p.y + size * 0.1, size * 0.22, size * 0.08, 0.0, Color::new(0.0, 0.0, 0.0, 0.25));
            vec2(p.x - size / 2.0, p.y - size * 0.8)
        }
        Stand::Afloat => vec2(p.x - size / 2.0, p.y - size * 0.55),
    };
    draw_texture_ex(
        &sheet,
        dest.x,
        dest.y,
        WHITE,
        DrawTextureParams { dest_size: Some(vec2(size, size)), source: Some(Rect::new(frame * n, row * n, n, n)), ..Default::default() },
    );
    true
}

/// A ship on the water without the install's sprites (a placeholder shape: hull, mast and a
/// sail of `sail` colour).
fn draw_ship(cam: &Camera, pos: (f32, f32), sail: Color) {
    let c = cam.to_screen(pos);
    let k = cam.scale / PX;
    let (hw, hh) = (18.0 * k, 7.0 * k);
    let hull = Color::new(0.42, 0.26, 0.12, 1.0);
    let (top, bottom) = (c.y - hh * 0.2, c.y + hh);
    draw_ellipse(c.x, bottom + 2.0 * k, hw * 1.1, 4.0 * k, 0.0, Color::new(0.0, 0.1, 0.2, 0.35));
    draw_rectangle(c.x - hw * 0.7, top, hw * 1.4, bottom - top, hull);
    draw_triangle(vec2(c.x - hw, top), vec2(c.x - hw * 0.7, top), vec2(c.x - hw * 0.7, bottom), hull);
    draw_triangle(vec2(c.x + hw, top), vec2(c.x + hw * 0.7, top), vec2(c.x + hw * 0.7, bottom), hull);
    draw_line(c.x, top, c.x, top - 30.0 * k, 2.0 * k.max(0.5), Color::new(0.3, 0.2, 0.1, 1.0));
    draw_triangle(vec2(c.x + 1.0, top - 28.0 * k), vec2(c.x + 1.0, top - 6.0 * k), vec2(c.x + 16.0 * k, top - 8.0 * k), sail);
}

fn draw_army(game: &Game, a: &Army, assets: &Assets, art: Option<&DtArt>, cam: &Camera) {
    let next = a.path.first().map(|&t| game.world.map.center(t));
    let pos = game.army_display_pos(a);
    if a.sails() {
        if !draw_figure(art, ship_stem(a.ship), pos, next, cam, Stand::Afloat) {
            let sail = if a.hostile() { Color::new(0.15, 0.12, 0.12, 1.0) } else { Color::new(0.92, 0.9, 0.82, 1.0) };
            draw_ship(cam, pos, sail);
        }
    } else if !draw_figure(art, figure_stem(a.model), pos, next, cam, Stand::Feet) {
        let c = cam.to_screen(pos);
        if let Some(leader) = a.leader() {
            assets.draw_unit(leader, if a.hostile() { Team::Enemy } else { Team::Player }, c.x, c.y - 8.0, 26.0);
        }
    }
    let c = cam.to_screen(pos);
    let ring = if a.hostile() { faction_color(4) } else { faction_color(a.faction) };
    draw_circle_lines(c.x, c.y + 4.0, 7.0 * cam.scale / PX + 3.0, 2.0, ring);
    if a.chasing {
        text_centered("!", c.x + 14.0, c.y - 30.0, 26.0, RED);
    }
}

fn draw_hero(game: &Game, assets: &Assets, art: Option<&DtArt>, cam: &Camera) {
    let model = match game.hero_class() {
        Some(HeroClass::Archmage) => 2,
        Some(HeroClass::Ranger) => 3,
        _ => 1,
    };
    let next = game.path.first().map(|&t| game.world.map.center(t));
    let c = cam.to_screen(game.display_pos());
    draw_circle(c.x, c.y + 4.0, 9.0 * cam.scale / PX + 3.0, Color::new(0.3, 0.9, 0.4, 0.35));
    if game.aboard() {
        if !draw_figure(art, ship_stem(razdor::rules::ships::kind::HERO), game.display_pos(), next, cam, Stand::Afloat) {
            draw_ship(cam, game.display_pos(), HERO_SAIL);
        }
        return;
    }
    if !draw_figure(art, figure_stem(model), game.display_pos(), next, cam, Stand::Feet) {
        assets.draw_unit(game.hero().def, Team::Player, c.x, c.y - 10.0, 30.0);
    }
}

fn draw_world(game: &Game, assets: &Assets, cam: &Camera) {
    let art = assets.dt.as_ref();
    draw_terrain(game, art, cam);
    // The route being walked lies on the ground, under the figures (the original shows no
    // preview before the click).
    if game.moving() {
        draw_route(game, &game.path, cam);
    }
    let map = &game.world.map;
    let ((c0, c1), (r0, r1)) = cam.visible(map);
    let rh = cam.grid.row_height();
    // Sprites stand on their cell and reach up to ~8 cells above it.
    let (below, side) = (10, 8);
    let mut items: Vec<(f32, Drawable)> = Vec::new();
    let fog = &game.fog;
    for o in map.objects_in_rows(r0 - 1, r1 + below) {
        if o.tile.0 >= c0 - side && o.tile.0 < c1 + side && fog.explored(o.tile) {
            items.push((o.tile.1 as f32 * rh, Drawable::Object(*o)));
        }
    }
    // Buildings stand in front of the scenery: hills, rocks and trees south of one would
    // hide it, so they are drawn after every object, sorted among themselves. Bridges lie
    // flat, under everything standing on them.
    let mut buildings: Vec<(f32, Drawable)> = Vec::new();
    for (i, l) in game.world.locations.iter().enumerate() {
        let (ax, ay) = l.anchor;
        let seen = l.cells().any(|t| fog.explored(t));
        if seen && ay >= r0 - 1 && ay < r1 + below && ax >= c0 - side && ax - l.size.0 < c1 + side {
            if l.kind.is_bridge() {
                items.push((ay as f32 * rh - 1000.0, Drawable::Building(i)));
            } else {
                buildings.push((ay as f32 * rh, Drawable::Building(i)));
            }
        }
    }
    // Figures (armies, the waiting ship, the hero) always stand in front of the scenery: they
    // are sorted among themselves and drawn after every object and building.
    let mut figures: Vec<(f32, Drawable)> = Vec::new();
    // Armies in the dark keep moving but are not shown.
    for (i, a) in game.world.armies.iter().enumerate().filter(|(_, a)| fog.explored(a.tile(map))) {
        figures.push((game.army_display_pos(a).1 + 0.02, Drawable::Army(i)));
    }
    if let Some(ship) = game.ship.filter(|s| !s.aboard && fog.explored(s.tile)) {
        figures.push((map.center(ship.tile).1 + 0.02, Drawable::Ship));
    }
    figures.push((game.display_pos().1 + 0.03, Drawable::Hero));
    items.sort_by(|a, b| a.0.total_cmp(&b.0));
    buildings.sort_by(|a, b| a.0.total_cmp(&b.0));
    figures.sort_by(|a, b| a.0.total_cmp(&b.0));
    for (_, d) in items.iter().chain(&buildings).chain(&figures) {
        match d {
            Drawable::Object(o) => draw_object(o, art, cam),
            Drawable::Building(i) => draw_building(&game.world.locations[*i], art, cam),
            Drawable::Army(i) => draw_army(game, &game.world.armies[*i], assets, art, cam),
            Drawable::Ship => {
                if let Some(ship) = game.ship {
                    let at = map.center(ship.tile);
                    if !draw_figure(art, ship_stem(razdor::rules::ships::kind::HERO), at, None, cam, Stand::Afloat) {
                        draw_ship(cam, at, HERO_SAIL);
                    }
                }
            }
            Drawable::Hero => draw_hero(game, assets, art, cam),
        }
    }
}

/// Route dots and, at the end, the travel time.
/// The route being walked, as the original draws it: a white arrow on every cell ahead
/// (`Windows/Way_Arrows.ugs`, 32×22, one frame per direction in the exe's order: up-left,
/// up, up-right, right, down-right, down, down-left, left). The time left is in the bar.
fn draw_route(game: &Game, path: &[Tile], cam: &Camera) {
    let arrows = super::chrome::animation("Windows/Way_Arrows.ugs").filter(|a| a.len() == 8);
    let zoom = cam.scale / PX;
    let mut from = game.tile();
    for &t in path {
        let c = cam.cell_centre(t);
        let (dx, dy) = ((t.0 - from.0).signum(), (t.1 - from.1).signum());
        from = t;
        let dir = match (dx, dy) {
            (-1, -1) => 0,
            (0, -1) => 1,
            (1, -1) => 2,
            (1, 0) => 3,
            (1, 1) => 4,
            (0, 1) => 5,
            (-1, 1) => 6,
            _ => 7,
        };
        match &arrows {
            Some(a) => {
                let (w, h) = (a[dir].width() * zoom, a[dir].height() * zoom);
                draw_texture_ex(&a[dir], c.x - w / 2.0, c.y - h / 2.0, WHITE, DrawTextureParams { dest_size: Some(vec2(w, h)), ..Default::default() });
            }
            None => {
                draw_circle(c.x, c.y, 3.5, Color::new(0.0, 0.0, 0.0, 0.5));
                draw_circle(c.x, c.y, 2.5, WHITE);
            }
        }
    }
}

/// A 2×6 (or 3×4) mini formation of portraits, the front row at the bottom as the enemy's
/// in battle; empty cells show their row's icon.
fn formation_grid(game: &Game, assets: &Assets, troops: &[Troop], team: Team, x: f32, y: f32, cell: f32) -> f32 {
    let f = game.content.formation;
    let lines = f.display_lines();
    for r in 0..lines {
        for col in 0..f.cols {
            let Some(slot) = f.at_display(r, col) else { continue };
            let (cx, cy) = (x + col as f32 * (cell + 3.0), y + (lines - 1 - r) as f32 * (cell + 3.0));
            let sq = Rect::new(cx, cy, cell, cell);
            draw_rectangle(cx, cy, cell, cell, Color::new(0.0, 0.0, 0.0, 0.45));
            draw_rectangle_lines(cx, cy, cell, cell, 1.0, Color::new(0.6, 0.6, 0.62, 0.8));
            let troop = troops.iter().find(|t| f.display(t.slot) == (r, col));
            if troop.is_none() {
                super::chrome::cell_icon(super::chrome::CellIcon::of(f, slot), sq);
            }
            if let Some(t) = troop {
                assets.draw_portrait(t.unit, team, sq);
                // Wounds left by world spells.
                let max = razdor::rules::units::Stats::of_level(&game.content, t.unit, t.level).max_hp();
                super::chrome::wounds(sq, max - t.hurt, max);
                draw_rectangle_lines(cx, cy, cell, cell, 1.0, Color::new(0.8, 0.8, 0.8, 0.9));
                // The troop's level in the corner.
                let lv = t.level.to_string();
                let tw = measure(&lv, 14.0).width;
                draw_rectangle(cx + cell - tw - 4.0, cy + cell - 14.0, tw + 4.0, 14.0, Color::new(0.0, 0.0, 0.0, 0.6));
                text(&lv, cx + cell - tw - 2.0, cy + cell - 2.0, 14.0, XP_COLOR);
            }
        }
    }
    f.display_lines() as f32 * (cell + 3.0)
}

struct Tooltip {
    title: String,
    lines: Vec<(String, Color)>,
    troops: Vec<Troop>,
    team: Team,
    footer: Vec<(String, Color)>,
}

/// A text of the install (`[Info] <key>`) in Russian, else ours.
fn info(key: &str, ours: &'static str) -> String {
    let t = (razdor::i18n::lang() == razdor::i18n::Lang::Ru).then(|| super::chrome::ui_text("Info", key)).flatten();
    t.unwrap_or_else(|| tr(ours).to_string())
}

/// The name colour of the original's tooltips (the leader, the owner).
const TIP_NAME: Color = Color::new(0.45, 1.0, 0.5, 1.0);
/// Its orange notes ("(дань уже собрана)").
const TIP_NOTE: Color = Color::new(1.0, 0.62, 0.25, 1.0);

/// The original's army tooltip: its name, its 2×6 cards, "Предводитель" and the leader's
/// name, the description; world spells on it and their wounds (Razdor's) under that.
fn army_tooltip(game: &Game, a: &Army) -> Tooltip {
    let title = if a.name.is_empty() { info("NoNameArmy", n_("Unknown army")) } else { a.name.clone() };
    let mut footer = Vec::new();
    if !a.leader_name.is_empty() {
        footer.push((info("Commander", n_("Leader")), DIM));
        footer.push((a.leader_name.clone(), TIP_NAME));
    }
    for line in wrap(&a.description, 320.0 * super::chrome::k(), 12.0 * super::chrome::k()) {
        footer.push((line, INK));
    }
    let now = game.clock.total_minutes() as u64;
    let spells: Vec<&str> = a.effects.iter().filter(|e| e.lasts_at(now)).filter_map(|e| game.spell(e.spell)).map(|s| s.name.as_str()).collect();
    if !spells.is_empty() {
        footer.push((trf!("Under spells: {spells}", spells = spells.join(", ")), MANA));
    }
    let hurt: i32 = a.troops.iter().map(|t| t.hurt).sum();
    if hurt > 0 {
        footer.push((trf!("Wounded by magic: -{hurt} hits", hurt), MANA));
    }
    Tooltip { title, lines: Vec::new(), troops: a.troops.clone(), team: if a.hostile() { Team::Enemy } else { Team::Player }, footer }
}

/// The original's building tooltip: its name, "Владелец" and the owner's name, the
/// description, "(дань уже собрана)" for a village already emptied; a garrison under
/// "Состав гарнизона защитников:".
fn location_tooltip(game: &Game, l: &Location) -> Tooltip {
    let title = if l.name.is_empty() { info("NoNameBuilding", n_("Unknown building")) } else { l.name.clone() };
    let mut lines = Vec::new();
    let owner = if l.owned() { game.hero_name.clone().unwrap_or_else(|| tr("you").to_string()) } else { l.owner_name.clone() };
    if !owner.trim().is_empty() {
        lines.push((info("Owner", n_("Owner")), DIM));
        lines.push((owner, TIP_NAME));
    }
    for line in wrap(&l.description, 320.0 * super::chrome::k(), 12.0 * super::chrome::k()) {
        lines.push((line, INK));
    }
    if l.kind == LocationKind::Village && l.tribute_gold <= 0 && l.tribute_mana <= 0 {
        lines.push((info("VillageEmptyGold", n_("(tribute already collected)")), TIP_NOTE));
    }
    let troops = if l.defended() { l.garrison.clone() } else { Vec::new() };
    if !troops.is_empty() {
        lines.push((info("Defenders", n_("The garrison's defenders:")), DIM));
    }
    Tooltip { title, lines, troops, team: Team::Enemy, footer: Vec::new() }
}

fn draw_tooltip(game: &Game, assets: &Assets, t: &Tooltip) {
    use super::chrome::{shadow_centered, CREAM};
    use super::dt_font::{with_face, Face};
    let k = super::chrome::k();
    let cell = (46.0 * k).round();
    let f = game.content.formation;
    // `formation_grid` spaces its cells 3 px apart.
    let grid_w = f.cols as f32 * (cell + 3.0) - 3.0;
    let grid_h = if t.troops.is_empty() { 0.0 } else { f.display_lines() as f32 * (cell + 3.0) + 8.0 * k };
    // Names in Benguiat, larger; the rest small.
    let big = |c: Color| c == TIP_NAME || c == TIP_NOTE;
    let size = |c: Color| if big(c) { 16.0 * k } else { 12.0 * k };
    let face = |c: Color| if big(c) { Face::Title } else { Face::Body };
    let shown = |c: Color| if c == INK { CREAM } else { c };
    let width = |s: &str, c: Color| with_face(face(c), || measure(s, size(c)).width);
    let title_size = 16.0 * k;
    let w = [with_face(Face::Title, || measure(&t.title, title_size).width) + 90.0 * k, grid_w + 24.0 * k, 250.0 * k]
        .into_iter()
        .chain(t.lines.iter().chain(&t.footer).map(|(s, c)| width(s, *c) + 24.0 * k))
        .fold(0.0, f32::max)
        .min(420.0 * k);
    let lh = |c: Color| size(c) + 3.0 * k;
    let lines_h: f32 = t.lines.iter().chain(&t.footer).map(|(_, c)| lh(*c)).sum();
    let bar = 24.0 * k;
    let h = bar + 8.0 * k + lines_h + grid_h + 8.0 * k;
    let (mx, my) = crate::ui::widgets::pointer();
    let x = (mx + 18.0).min(screen_width() - w - 4.0);
    let y = (my + 18.0).min(screen_height() - bar_h() - h - 4.0).max(2.0);
    tooltip_panel(Rect::new(x, y, w, h));
    // The title strip with the ornaments at its ends.
    draw_rectangle(x + 2.0, y + 2.0, w - 4.0, bar - 2.0, Color::new(0.0, 0.0, 0.0, 0.25));
    if let Some(orn) = super::chrome::win_fx("Corner-Left", super::chrome::Fx::KeyBlack) {
        let oh = bar * 0.8;
        let ow = orn.width() * oh / orn.height();
        let tint = Color::new(0.55, 0.8, 0.7, 0.8);
        super::chrome::tex(&orn, Rect::new(x + 4.0 * k, y + (bar - oh) / 2.0, ow, oh), tint);
        if let Some(r) = super::chrome::win_fx("Corner-Right", super::chrome::Fx::KeyBlack) {
            super::chrome::tex(&r, Rect::new(x + w - ow - 4.0 * k, y + (bar - oh) / 2.0, ow, oh), tint);
        }
    }
    draw_line(x + 2.0, y + bar, x + w - 2.0, y + bar, 1.0, super::chrome::SILVER);
    with_face(Face::Title, || shadow_centered(&t.title, x + w / 2.0, y + bar * 0.5 + title_size * 0.36, title_size, CREAM));
    let mut ly = y + bar + 6.0 * k;
    let line = |s: &str, c: Color, ly: &mut f32| {
        with_face(face(c), || shadow_centered(s, x + w / 2.0, *ly + size(c), size(c), shown(c)));
        *ly += lh(c);
    };
    for (s, c) in &t.lines {
        line(s, *c, &mut ly);
    }
    if !t.troops.is_empty() {
        ly += formation_grid(game, assets, &t.troops, t.team, x + (w - grid_w) / 2.0, ly + 4.0 * k, cell) + 8.0 * k;
    }
    for (s, c) in &t.footer {
        line(s, *c, &mut ly);
    }
}

/// What the mouse is over: an army, else a building.
fn hover_tooltip(game: &Game, cam: &Camera) -> Option<Tooltip> {
    let m = Vec2::from(crate::ui::widgets::pointer());
    if !cam.view.contains(m) {
        return None;
    }
    let near = 22.0 * (cam.scale / PX).max(0.6);
    let map = &game.world.map;
    if let Some(a) = game.world.armies.iter().filter(|a| game.fog.explored(a.tile(map))).find(|a| (cam.to_screen(game.army_display_pos(a)) - vec2(0.0, 12.0 * cam.scale / PX) - m).length() < near) {
        return Some(army_tooltip(game, a));
    }
    let t = cam.tile_under_mouse().filter(|&t| game.fog.explored(t))?;
    let l = game.world.location_covering(t).or_else(|| game.world.location_at(t))?;
    Some(location_tooltip(game, &game.world.locations[l]))
}

/// A click on the building the party stands in (`t` one of its cells): its window again, or
/// the battle with a garrison still to beat. Nothing for a burnt camp.
fn reopen_here(game: &mut Game, t: Tile) -> Option<Screen> {
    let l = game.location?;
    if game.world.location_covering(t).or_else(|| game.world.location_at(t)) != Some(l) {
        return None;
    }
    let loc = &game.world.locations[l];
    if loc.kind == LocationKind::Camp && loc.cleared {
        return None;
    }
    if loc.defended() {
        game.foe = Some(Foe::Garrison(l));
        return Some(saves::battle(game));
    }
    first_tab(loc).map(|first| Screen::Building(BuildingView::new(first)))
}


fn describe(event: &Event, game: &Game) -> Option<String> {
    match event {
        Event::NewDay(_) | Event::Captured(_) | Event::Script(_) => None,
        Event::Tribute { paid, mana, .. } => Some(match paid {
            razdor::rules::game::Tribute::Gold(g) => trf!("The village pays its tribute: {g} gold and {mana} mana.", g, mana),
            razdor::rules::game::Tribute::Item(item) => trf!("The village pays with a {item} and {mana} mana.", item = game.content.item(*item).name, mana),
        }),
        Event::LevelUp(i, level) => game.squad.get(*i).map(|u| trf!("{name} reaches level {level}!", name = u.name(&game.content), level)),
        Event::Battle(news) => Some(news.text.clone()),
        Event::Arrived(l) => {
            let loc = &game.world.locations[*l];
            game.foe.is_some().then(|| trf!("{place}: the garrison bars your way!", place = loc.name))
        }
        Event::Encounter(i) => {
            let a = &game.world.armies[*i];
            Some(if a.name.is_empty() { tr("An army attacks!").to_string() } else { trf!("{name} attacks!", name = a.name) })
        }
        Event::Met(i) => {
            let a = &game.world.armies[*i];
            let who = if a.name.is_empty() { tr("An army") } else { a.name.as_str() };
            Some(trf!("A meeting on the road: {who} lets you pass.", who))
        }
    }
}

/// Applies the events of a tick or a wait: noon reports open the report window, stepping
/// into a building opens its window, the scenario's events open their dialogs. Returns the
/// next screen, if any.
pub(super) fn handle_events(game: &mut Game, events: Vec<Event>, message: &mut Option<String>, dialogs: &mut VecDeque<Dialog>) -> Option<Screen> {
    let mut next = None;
    for event in events {
        if let Some(m) = describe(&event, game) {
            *message = Some(m);
        }
        match event {
            Event::Encounter(_) => next = Some(saves::battle(game)),
            Event::Arrived(l) => {
                if game.foe.is_some() {
                    next = Some(saves::battle(game));
                } else if let Some(first) = first_tab(&game.world.locations[l]) {
                    *message = None;
                    next = Some(Screen::Building(BuildingView::new(first)));
                }
            }
            // A noon when no money came in or went out has nothing to report.
            Event::NewDay(r) if r.is_empty() => {}
            Event::NewDay(r) => dialogs.push_back(Dialog::day_report(game, &r)),
            Event::Captured(l) => dialogs.push_back(Dialog::captured(game, l)),
            Event::Met(_) | Event::Battle(_) | Event::Tribute { .. } => {}
            Event::LevelUp(..) => cue(Cue::Upgrade),
            Event::Script(o) => story::show(game, &o, message, dialogs),
        }
    }
    next
}

/// The map under a building window or a dialog: drawn over the whole screen, not
/// interactive, with the bar's buttons greyed (`lit`: the open screen's button).
pub fn backdrop_lit(game: &Game, assets: &Assets, lit: Option<BarButton>) {
    clear_background(rgb(10, 12, 10));
    let full = Rect::new(0.0, 0.0, screen_width(), screen_height() - bar_h());
    let cam = Camera::looking_in(game, 1.0, game.display_pos(), full);
    draw_world(game, assets, &cam);
    cam.draw_fog(game);
    draw_rectangle(0.0, 0.0, screen_width(), screen_height(), Color::new(0.0, 0.0, 0.0, 0.2));
    game_bar::draw(game, |b| if Some(b) == lit { Look::Lit } else { Look::Grey });
}

pub fn backdrop(game: &Game, assets: &Assets) {
    backdrop_lit(game, assets, None);
}

/// The map under a window the bar stays live for (army, spell book, journal, a building),
/// as in the original: its buttons are blue, the window's own one green; one pressed gives
/// the screen it opens (the lit one, or the map button, closes the window).
pub fn window_backdrop(game: &Game, assets: &Assets, lit: Option<BarButton>) -> Option<Screen> {
    clear_background(rgb(10, 12, 10));
    let full = Rect::new(0.0, 0.0, screen_width(), screen_height() - bar_h());
    let cam = Camera::looking_in(game, 1.0, game.display_pos(), full);
    draw_world(game, assets, &cam);
    cam.draw_fog(game);
    draw_rectangle(0.0, 0.0, screen_width(), screen_height(), Color::new(0.0, 0.0, 0.0, 0.2));
    let idle = game.foe.is_none();
    let modal = input_blocked();
    let pressed = game_bar::draw(game, |b| match b {
        _ if modal => Look::Grey,
        _ if Some(b) == lit => Look::Lit,
        BarButton::Save | BarButton::Spells if !idle => Look::Grey,
        _ => Look::Normal,
    })?;
    Some(match pressed {
        b if Some(b) == lit => Screen::WorldMap,
        BarButton::Menu => Screen::Menu(false),
        BarButton::Settings => Screen::Settings,
        BarButton::Save => Screen::Save(SaveView::new(game, Back::Map)),
        BarButton::Load => Screen::Load(LoadView::new(Back::Map)),
        BarButton::Journal => Screen::Journal(Default::default()),
        BarButton::Squad => Screen::Squad { selected: 0, scroll: 0, back: None },
        BarButton::Spells => Screen::Spellbook { selected: 0 },
        BarButton::Map => Screen::WorldMap,
    })
}

/// The bottom bar of the map: its buttons and keys. Returns the next screen and whether the
/// minimap was toggled.
fn bottom_bar(game: &mut Game, message: &mut Option<String>, minimap_open: bool) -> (Option<Screen>, bool) {
    let idle = game.foe.is_none();
    let modal = input_blocked();
    let look = |b: BarButton| match b {
        _ if modal => Look::Grey,
        BarButton::Save | BarButton::Spells if !idle => Look::Grey,
        BarButton::Map if minimap_open => Look::Glow,
        _ => Look::Normal,
    };
    let mut pressed = game_bar::draw(game, look);
    if pressed.is_none() {
        // Esc closes the minimap first; the menu only when nothing else is open.
        pressed = if key(KeyCode::Escape) {
            Some(if minimap_open { BarButton::Map } else { BarButton::Menu })
        } else if idle && key(KeyCode::B) {
            Some(BarButton::Spells)
        } else if key(KeyCode::J) {
            Some(BarButton::Journal)
        } else if key(KeyCode::A) {
            Some(BarButton::Squad)
        } else {
            None
        };
    }
    let next = match pressed {
        Some(BarButton::Menu) => Some(Screen::Menu(false)),
        Some(BarButton::Settings) => Some(Screen::Settings),
        Some(BarButton::Save) => Some(Screen::Save(SaveView::new(game, Back::Map))),
        Some(BarButton::Load) => Some(Screen::Load(LoadView::new(Back::Map))),
        Some(BarButton::Journal) => Some(Screen::Journal(Default::default())),
        Some(BarButton::Squad) => {
            *message = None;
            Some(Screen::Squad { selected: 0, scroll: 0, back: None })
        }
        Some(BarButton::Spells) => {
            *message = None;
            Some(Screen::Spellbook { selected: 0 })
        }
        Some(BarButton::Map) | None => None,
    };
    if next.is_some() {
        game.stop();
    }
    (next, pressed == Some(BarButton::Map))
}

pub fn frame(game: &mut Game, assets: &Assets, view: &mut MapView, message: &mut Option<String>, dialogs: &mut VecDeque<Dialog>) -> Option<Screen> {
    clear_background(rgb(10, 12, 10));

    // Zoom: mouse wheel or +/-.
    let wheel = wheel();
    if wheel != 0.0 {
        view.zoom = (view.zoom * if wheel > 0.0 { 1.1 } else { 1.0 / 1.1 }).clamp(0.4, 2.0);
    }
    if key(KeyCode::Equal) || key(KeyCode::KpAdd) {
        view.zoom = (view.zoom * 1.2).min(2.0);
    }
    if key(KeyCode::Minus) || key(KeyCode::KpSubtract) {
        view.zoom = (view.zoom / 1.2).max(0.4);
    }

    // Input: click to walk, right click or Space to stop; M toggles the minimap.
    if key(KeyCode::M) {
        view.minimap = !view.minimap;
    }
    // Places the scenario has just shown wait in line (dark until their turn). Tab or a
    // click on the map skips the showing; Tab: the camera back on the hero (after the
    // minimap or a showing moved it).
    for shown in std::mem::take(&mut game.shown) {
        view.shows.push_back(Showing::new(game, &shown));
    }
    if !view.shows.is_empty() && !input_blocked() && (clicked() || key(KeyCode::Tab)) {
        view.shows.clear();
    }
    if key(KeyCode::Tab) {
        view.look = None;
    }
    let cam = Camera::looking_at(game, view.zoom, view.look.unwrap_or(game.display_pos()));
    let on_minimap = view.minimap && minimap::outer(&game.world.map, cam.view).contains(Vec2::from(crate::ui::widgets::pointer()));
    let hovered = cam.tile_under_mouse().filter(|_| !on_minimap);
    let mut reopened = None;
    if clicked() && !on_minimap {
        if let Some(screen) = hovered.and_then(|t| reopen_here(game, t)) {
            // A click on the building the party stands in opens it again.
            *message = None;
            reopened = Some(screen);
        } else if let Some(t) = hovered {
            // Water is sailed to with a ship; otherwise a click next to open ground means it.
            let target = if game.can_sail_to(t) {
                t
            } else {
                game.world.map.nearest_passable(t, 1).filter(|_| game.world.location_covering(t).is_none()).unwrap_or(t)
            };
            if target != game.tile() && !game.set_destination(target) {
                *message = Some(tr("No way through.").into());
            }
            view.look = None;
        }
    }
    if right_clicked() || key(KeyCode::Space) {
        game.stop();
    }

    // Time stands still while a window is open.
    let mut events = game.drain_events();
    if !input_blocked() && events.is_empty() {
        events = game.tick(get_frame_time().min(0.1));
    }
    let mut next = handle_events(game, events, message, dialogs).or(reopened);

    // The place being shown: once its message is read, the camera flies there and the
    // uncovered area fades in; then the next place, if any. The camera stays on the last
    // one until a click on the map or Tab brings it back to the hero.
    let now = get_time();
    for shown in std::mem::take(&mut game.shown) {
        view.shows.push_back(Showing::new(game, &shown));
    }
    if dialogs.is_empty() {
        if let Some(front) = view.shows.front_mut() {
            let here = view.look.unwrap_or(game.display_pos());
            let (t0, from) = *front.started.get_or_insert((now, here));
            let p = ((now - t0) / SHOW_PAN).clamp(0.0, 1.0) as f32;
            let ease = p * p * (3.0 - 2.0 * p);
            view.look = Some((from.0 + (front.at.0 - from.0) * ease, from.1 + (front.at.1 - from.1) * ease));
            if now - t0 > SHOW_PAN + SHOW_FADE + SHOW_REST {
                view.shows.pop_front();
            }
        }
    }

    let cam = Camera::looking_at(game, view.zoom, view.look.unwrap_or(game.display_pos()));
    draw_world(game, assets, &cam);
    cam.draw_fog(game);
    cam.draw_showing(game, &view.shows, now);

    // The original has no side panel: the map fills the screen above the bar. Lasting
    // world spells and ship hints stand small in the top left corner.
    let now = game.clock.total_minutes() as u64;
    let mut notes: Vec<(String, Color)> = game
        .active_spells()
        .iter()
        .filter_map(|e| {
            let name = &game.spell(e.spell)?.name;
            Some(match e.until {
                Some(t) => format!("{name} ({})", duration_label(t.saturating_sub(now) as f64)),
                None => name.clone(),
            })
        })
        .map(|l| (l, MANA))
        .collect();
    if game.aboard() {
        notes.push((tr("At sea: click the shore to land.").into(), ACCENT));
    } else if game.ship.is_some() {
        notes.push((tr("Your ship waits; walk onto it to sail.").into(), ACCENT));
    }
    for (i, (line, color)) in notes.iter().enumerate() {
        super::chrome::shadow_text(line, 10.0, 22.0 + i as f32 * 18.0, 16.0, *color);
    }
    // Waiting: 1 / 4, or a click on the time panel (left 1 h, right 4 h). Waits play in real
    // time, a 30-minute tick every 150 ms (`Game::tick`).
    let can_wait = game.foe.is_none() && !game.waiting();
    let clock = game_bar::time_panel();
    let on_clock = !input_blocked() && clock.contains(crate::ui::widgets::pointer().into());
    if can_wait && (key(KeyCode::Key1) || (on_clock && clicked())) {
        game.begin_wait(1);
    }
    if can_wait && (key(KeyCode::Key4) || (on_clock && right_clicked())) {
        game.begin_wait(4);
    }

    let (bar, toggle_map) = bottom_bar(game, message, view.minimap);
    next = next.or(bar);
    if toggle_map {
        view.minimap = !view.minimap;
    }
    if view.minimap {
        if let Some(at) = minimap::window(game, assets.dt.as_ref(), cam.view, cam.world_rect(), surface_color) {
            view.look = Some(at);
        }
    }
    if let Some(t) = hover_tooltip(game, &cam).filter(|_| !on_minimap) {
        draw_tooltip(game, assets, &t);
    }
    if on_clock && can_wait {
        let hint = |key: &str, ours: &'static str| super::chrome::ui_text("GameMenu", key).filter(|_| razdor::i18n::lang() == razdor::i18n::Lang::Ru).unwrap_or_else(|| tr(ours).to_string());
        let left = hint("cp_Wait1Hour", n_("Wait 1 hour (the hero stands still)"));
        let right = hint("cp_Wait4Hour", n_("Wait 4 hours (the hero stands still)"));
        tooltip(&[(trf!("Left click: {left}", left), INK), (trf!("Right click: {right}", right), INK)]);
    }
    if let Some(m) = message {
        let w = measure(m, 22.0).width + 40.0;
        let (cx, y) = (screen_width() / 2.0, screen_height() - bar_h() - 50.0);
        draw_rectangle(cx - w / 2.0, y, w, 36.0, PANEL);
        text_centered(m, cx, y + 25.0, 22.0, ACCENT);
    }
    next
}
