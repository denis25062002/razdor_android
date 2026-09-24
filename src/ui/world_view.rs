//! The world map: terrain, objects, buildings, armies, the party, its clock and money.
//!
//! With a Discord Times install the original terrain textures, map objects, buildings and
//! map figures are drawn (decoded at runtime by [`DtArt`]); otherwise coloured cells and
//! simple shapes. Only the visible cells are drawn, so 200×200 maps stay fast.

use std::collections::VecDeque;

use macroquad::prelude::*;

use razdor::rules::battle::Team;
use razdor::rules::clock::duration_label;
use razdor::rules::content::HeroClass;
use razdor::rules::formation::Row;
use razdor::rules::game::{Event, Foe, Game};
use razdor::rules::town::first_tab;
use razdor::rules::map::{object_class, Decoration, Grid, Tile, TileMap};
use razdor::rules::world::{Army, Location, LocationKind, Troop};

use super::assets::Assets;
use super::battle_view::BattleView;
use super::building_view::BuildingView;
use super::dialog::Dialog;
use super::dt_art::DtArt;
use super::minimap;
use super::screens::squad_panel;
use super::widgets::*;
use super::Screen;

/// Screen pixels per world unit (one cell width) at zoom 1: the original's 32 px cells.
const PX: f32 = 32.0;
const PANEL_W: f32 = 270.0;
const BAR_H: f32 = 84.0;
use super::dialog::MANA;

/// World-map view state kept between frames.
pub struct MapView {
    pub zoom: f32,
    /// Hovered target cell, its path from the party and the travel time (minutes).
    preview: Option<(Tile, Tile, Vec<Tile>, f32)>,
    /// The minimap window is open.
    pub minimap: bool,
    /// Where the camera looks when moved by the minimap (world units); `None` follows the hero.
    pub look: Option<(f32, f32)>,
}

impl Default for MapView {
    fn default() -> Self {
        MapView { zoom: 1.0, preview: None, minimap: false, look: None }
    }
}

impl MapView {
    pub fn reset(&mut self) {
        self.preview = None;
        self.look = None;
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
fn faction_color(faction: u8) -> Color {
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
    fn follow(game: &Game, zoom: f32) -> Camera {
        Camera::looking_at(game, zoom, game.pos)
    }

    /// Centred on world position `at` (clamped to the map).
    fn looking_at(game: &Game, zoom: f32, at: (f32, f32)) -> Camera {
        let view = Rect::new(0.0, 0.0, screen_width() - PANEL_W, screen_height() - BAR_H);
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

    /// The fog layer over the whole map (`ui::minimap`).
    fn draw_fog(&self, game: &Game) {
        let map = &game.world.map;
        let rh = map.grid.row_height();
        let tl = self.to_screen((-0.5, -0.5 * rh));
        let br = self.to_screen((map.w as f32 - 0.5, (map.h as f32 - 0.5) * rh));
        minimap::draw_fog(&game.fog, tl, br);
    }

    fn tile_under_mouse(&self) -> Option<Tile> {
        let m = Vec2::from(mouse_position());
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
fn draw_wrapped(tex: &Texture2D, dest: Rect, src: Vec2, src_size: Vec2) {
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
fn figure_stem(model: u8) -> &'static str {
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

/// Draws a map figure standing at world `pos`, heading towards `next`. Returns false if the
/// install has no such figure.
fn draw_figure(art: Option<&DtArt>, stem: &str, pos: (f32, f32), next: Option<(f32, f32)>, cam: &Camera) -> bool {
    let Some(sheet) = art.and_then(|a| a.figure_sheet(stem)) else { return false };
    let p = cam.to_screen(pos);
    let heading = next.map_or(Vec2::ZERO, |n| cam.to_screen(n) - p);
    let row = facing_row(heading);
    let frame = if next.is_some() { ((get_time() * 10.0) as i32 % 8) as f32 } else { 0.0 };
    let size = 64.0 * cam.scale / PX;
    let dest = vec2(p.x - size / 2.0, p.y - size * 0.8);
    draw_ellipse(p.x, p.y + size * 0.1, size * 0.22, size * 0.08, 0.0, Color::new(0.0, 0.0, 0.0, 0.25));
    draw_texture_ex(
        &sheet,
        dest.x,
        dest.y,
        WHITE,
        DrawTextureParams { dest_size: Some(vec2(size, size)), source: Some(Rect::new(frame * 64.0, row * 64.0, 64.0, 64.0)), ..Default::default() },
    );
    true
}

fn draw_army(game: &Game, a: &Army, assets: &Assets, art: Option<&DtArt>, cam: &Camera) {
    let next = a.path.first().map(|&t| game.world.map.center(t));
    if !draw_figure(art, figure_stem(a.model), a.pos, next, cam) {
        let c = cam.to_screen(a.pos);
        if let Some(leader) = a.leader() {
            assets.draw_unit(leader, if a.hostile() { Team::Enemy } else { Team::Player }, c.x, c.y - 8.0, 26.0);
        }
    }
    let c = cam.to_screen(a.pos);
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
    let c = cam.to_screen(game.pos);
    draw_circle(c.x, c.y + 4.0, 9.0 * cam.scale / PX + 3.0, Color::new(0.3, 0.9, 0.4, 0.35));
    if !draw_figure(art, figure_stem(model), game.pos, next, cam) {
        assets.draw_unit(game.hero().def, Team::Player, c.x, c.y - 10.0, 30.0);
    }
}

fn draw_world(game: &Game, assets: &Assets, cam: &Camera) {
    let art = assets.dt.as_ref();
    draw_terrain(game, art, cam);
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
    for (i, l) in game.world.locations.iter().enumerate() {
        let (ax, ay) = l.anchor;
        let seen = l.cells().any(|t| fog.explored(t));
        if seen && ay >= r0 - 1 && ay < r1 + below && ax >= c0 - side && ax - l.size.0 < c1 + side {
            // Bridges lie flat: under everything standing on them.
            let key = if l.kind.is_bridge() { ay as f32 * rh - 1000.0 } else { ay as f32 * rh + 0.01 };
            items.push((key, Drawable::Building(i)));
        }
    }
    // Armies in the dark keep moving but are not shown.
    for (i, a) in game.world.armies.iter().enumerate().filter(|(_, a)| fog.explored(a.tile(map))) {
        items.push((a.pos.1 + 0.02, Drawable::Army(i)));
    }
    items.push((game.pos.1 + 0.03, Drawable::Hero));
    items.sort_by(|a, b| a.0.total_cmp(&b.0));
    for (_, d) in &items {
        match d {
            Drawable::Object(o) => draw_object(o, art, cam),
            Drawable::Building(i) => draw_building(&game.world.locations[*i], art, cam),
            Drawable::Army(i) => draw_army(game, &game.world.armies[*i], assets, art, cam),
            Drawable::Hero => draw_hero(game, assets, art, cam),
        }
    }
}

/// Route dots and, at the end, the travel time.
fn draw_route(game: &Game, path: &[Tile], minutes: f32, cam: &Camera, color: Color) {
    for (k, &t) in path.iter().enumerate() {
        let c = cam.cell_centre(t);
        let r = if k + 1 == path.len() { 5.0 } else { 2.5 };
        draw_circle(c.x, c.y, r + 1.0, Color::new(0.0, 0.0, 0.0, 0.5));
        draw_circle(c.x, c.y, r, color);
    }
    if let Some(&last) = path.last() {
        let c = cam.cell_centre(last);
        let label = duration_label(minutes as f64);
        let w = measure(&label, 18.0).width + 12.0;
        draw_rectangle(c.x + 8.0, c.y - 26.0, w, 22.0, PANEL);
        text(&label, c.x + 14.0, c.y - 10.0, 18.0, ACCENT);
    }
    let _ = game;
}

/// A 2×6 (or 3×4) mini formation of portraits.
fn formation_grid(game: &Game, assets: &Assets, troops: &[Troop], team: Team, x: f32, y: f32, cell: f32) -> f32 {
    let f = game.content.formation;
    for (r, &row) in f.rows().iter().enumerate() {
        for col in 0..f.cols {
            let (cx, cy) = (x + col as f32 * (cell + 3.0), y + r as f32 * (cell + 3.0));
            draw_rectangle(cx, cy, cell, cell, Color::new(0.0, 0.0, 0.0, 0.35));
            draw_rectangle_lines(cx, cy, cell, cell, 1.0, if row == Row::Front { DIM } else { Color::new(0.4, 0.4, 0.4, 1.0) });
            if let Some(t) = troops.iter().find(|t| t.slot.row == row && t.slot.col == col) {
                assets.draw_unit(t.unit, team, cx + cell / 2.0, cy + cell / 2.0, cell);
            }
        }
    }
    f.rows().len() as f32 * (cell + 3.0)
}

struct Tooltip {
    title: String,
    lines: Vec<(String, Color)>,
    troops: Vec<Troop>,
    team: Team,
    footer: Vec<(String, Color)>,
}

fn army_tooltip(a: &Army) -> Tooltip {
    let title = if a.name.is_empty() { "Army".to_string() } else { a.name.clone() };
    let mut footer = Vec::new();
    if !a.leader_name.is_empty() {
        footer.push(("Leader".to_string(), DIM));
        footer.push((a.leader_name.clone(), GREEN));
    }
    let stance = if a.hostile() { ("Hostile: attacks on sight", RED) } else { ("Not hostile", DIM) };
    footer.push((stance.0.to_string(), stance.1));
    for line in wrap(&a.description, 330.0, 16.0).into_iter().take(4) {
        footer.push((line, INK));
    }
    Tooltip { title, lines: Vec::new(), troops: a.troops.clone(), team: if a.hostile() { Team::Enemy } else { Team::Player }, footer }
}

fn location_tooltip(game: &Game, l: &Location) -> Tooltip {
    let title = if l.name.is_empty() { l.kind.label().to_string() } else { l.name.clone() };
    let mut lines = vec![(l.kind.label().to_string(), DIM)];
    if l.owned() {
        lines.push(("Owner: you".to_string(), GREEN));
    } else if !l.owner_name.is_empty() {
        lines.push((format!("Owner: {}", l.owner_name), INK));
    }
    if l.hostile() {
        lines.push(("Hostile".to_string(), RED));
    }
    match l.kind {
        LocationKind::Village if l.tribute_gold > 0 || l.tribute_mana > 0 => {
            lines.push((format!("Tribute waiting: {} gold, {} mana", l.tribute_gold, l.tribute_mana), ACCENT))
        }
        LocationKind::Village => lines.push(("(tribute already collected)".to_string(), rgb(240, 150, 60))),
        _ if l.gold_income > 0 || l.mana_income > 0 => {
            lines.push((format!("Income {} gold, {} mana a day", l.gold_income, l.mana_income), ACCENT))
        }
        _ => {}
    }
    let mut footer = Vec::new();
    for line in wrap(&l.description, 330.0, 16.0).into_iter().take(5) {
        footer.push((line, INK));
    }
    let troops = if l.defended() { l.garrison.clone() } else { Vec::new() };
    let _ = game;
    Tooltip { title, lines, troops, team: Team::Enemy, footer }
}

fn draw_tooltip(game: &Game, assets: &Assets, t: &Tooltip) {
    let cell = 30.0;
    let f = game.content.formation;
    let grid_w = f.cols as f32 * (cell + 3.0);
    let grid_h = if t.troops.is_empty() { 0.0 } else { f.rows().len() as f32 * (cell + 3.0) + 8.0 };
    let w = [measure(&t.title, 22.0).width + 24.0, grid_w + 24.0, 250.0]
        .into_iter()
        .chain(t.lines.iter().chain(&t.footer).map(|(s, _)| measure(s, 16.0).width + 24.0))
        .fold(0.0, f32::max)
        .min(380.0);
    let h = 34.0 + (t.lines.len() + t.footer.len()) as f32 * 19.0 + grid_h + 8.0;
    let (mx, my) = mouse_position();
    let x = (mx + 18.0).min(screen_width() - w - 4.0);
    let y = (my + 18.0).min(screen_height() - h - 4.0);
    draw_rectangle(x, y, w, h, Color::new(0.06, 0.12, 0.09, 0.93));
    draw_rectangle_lines(x, y, w, h, 2.0, Color::new(0.35, 0.55, 0.4, 1.0));
    text_centered(&t.title, x + w / 2.0, y + 24.0, 22.0, ACCENT);
    let mut ly = y + 34.0;
    for (s, c) in &t.lines {
        text_centered(s, x + w / 2.0, ly + 14.0, 16.0, *c);
        ly += 19.0;
    }
    if !t.troops.is_empty() {
        ly += formation_grid(game, assets, &t.troops, t.team, x + (w - grid_w) / 2.0, ly + 4.0, cell) + 8.0;
    }
    for (s, c) in &t.footer {
        text_centered(s, x + w / 2.0, ly + 14.0, 16.0, *c);
        ly += 19.0;
    }
}

/// What the mouse is over: an army, else a building.
fn hover_tooltip(game: &Game, cam: &Camera) -> Option<Tooltip> {
    let m = Vec2::from(mouse_position());
    if !cam.view.contains(m) {
        return None;
    }
    let near = 22.0 * (cam.scale / PX).max(0.6);
    let map = &game.world.map;
    if let Some(a) = game.world.armies.iter().filter(|a| game.fog.explored(a.tile(map))).find(|a| (cam.to_screen(a.pos) - vec2(0.0, 12.0 * cam.scale / PX) - m).length() < near) {
        return Some(army_tooltip(a));
    }
    let t = cam.tile_under_mouse().filter(|&t| game.fog.explored(t))?;
    let l = game.world.location_covering(t).or_else(|| game.world.location_at(t))?;
    Some(location_tooltip(game, &game.world.locations[l]))
}

/// The location the party stands on: enter it, or attack its garrison.
fn location_panel(game: &mut Game, x: f32, mut y: f32) -> Option<Screen> {
    let Some(l) = game.location else {
        text("On the road.", x, y + 20.0, 20.0, DIM);
        return None;
    };
    let loc = &game.world.locations[l];
    let name = if loc.name.is_empty() { loc.kind.label().to_string() } else { loc.name.clone() };
    for line in wrap(&name, PANEL_W - 30.0, 22.0).iter().take(2) {
        text(line, x, y + 20.0, 22.0, INK);
        y += 24.0;
    }
    text(loc.kind.label(), x, y + 14.0, 16.0, DIM);
    y += 26.0;
    if loc.kind == LocationKind::Camp && loc.cleared {
        text("Only ashes remain.", x, y + 14.0, 17.0, DIM);
    } else if loc.defended() {
        if button(x, y, 240.0, 44.0, "Attack the garrison", true) {
            game.foe = Some(Foe::Garrison(l));
            return Some(Screen::Battle(Box::new(BattleView::new(game.start_battle()))));
        }
    } else if let Some(first) = first_tab(loc) {
        if button(x, y, 240.0, 44.0, "Enter", true) {
            return Some(Screen::Building(BuildingView::new(first)));
        }
        if loc.kind == LocationKind::Village {
            let status = if game.tribute_available().is_some() { "Tribute is waiting." } else { "Tribute already collected." };
            text(status, x, y + 64.0, 16.0, DIM);
        }
    }
    None
}

fn describe(event: &Event, game: &Game) -> Option<String> {
    match event {
        Event::NewDay(_) | Event::Captured(_) => None,
        Event::Arrived(l) => {
            let loc = &game.world.locations[*l];
            game.foe.is_some().then(|| format!("{}: the garrison bars your way!", loc.name))
        }
        Event::Encounter(i) => {
            let a = &game.world.armies[*i];
            Some(if a.name.is_empty() { "An army attacks!".to_string() } else { format!("{} attacks!", a.name) })
        }
        Event::Met(i) => {
            let a = &game.world.armies[*i];
            let who = if a.name.is_empty() { "An army" } else { a.name.as_str() };
            Some(format!("A meeting on the road: {who} lets you pass."))
        }
    }
}

/// Applies the events of a tick or a wait: noon reports open the report window, stepping
/// into a building opens its window. Returns the next screen, if any.
fn handle_events(game: &mut Game, events: Vec<Event>, message: &mut Option<String>, dialogs: &mut VecDeque<Dialog>) -> Option<Screen> {
    let mut next = None;
    for event in events {
        if let Some(m) = describe(&event, game) {
            *message = Some(m);
        }
        match event {
            Event::Encounter(_) => next = Some(Screen::Battle(Box::new(BattleView::new(game.start_battle())))),
            Event::Arrived(l) => {
                if game.foe.is_some() {
                    next = Some(Screen::Battle(Box::new(BattleView::new(game.start_battle()))));
                } else if let Some(first) = first_tab(&game.world.locations[l]) {
                    *message = None;
                    next = Some(Screen::Building(BuildingView::new(first)));
                }
            }
            Event::NewDay(r) => dialogs.push_back(Dialog::day_report(game, &r)),
            Event::Captured(l) => dialogs.push_back(Dialog::captured(game, l)),
            Event::Met(_) => {}
        }
    }
    next
}

/// Gold, mana, income and wages along the bottom edge.
fn resource_strip(game: &Game) {
    let (w, h) = (screen_width(), screen_height());
    let sy = h - 10.0;
    let mut wages = format!("wages -{}", game.daily_wages());
    if game.daily_mana_wages() > 0 {
        wages += &format!(" / -{} mana", game.daily_mana_wages());
    }
    let items: [(String, Color); 4] = [
        (format!("mana {}", game.mana), MANA),
        (format!("gold {}", game.gold), ACCENT),
        (format!("income +{}", game.daily_income()), INK),
        (wages, rgb(240, 150, 60)),
    ];
    for (i, (s, c)) in items.iter().enumerate() {
        text(s, 20.0 + i as f32 * (w - 40.0) / 4.0, sy, 20.0, *c);
    }
}

/// The map under a building window or a dialog: drawn, not interactive.
pub fn backdrop(game: &Game, assets: &Assets) {
    clear_background(rgb(10, 12, 10));
    let cam = Camera::follow(game, 1.0);
    draw_world(game, assets, &cam);
    cam.draw_fog(game);
    draw_rectangle(0.0, 0.0, screen_width(), screen_height(), Color::new(0.0, 0.0, 0.0, 0.3));
    let (w, h) = (screen_width(), screen_height());
    draw_rectangle(0.0, h - BAR_H, w, BAR_H, Color::new(0.08, 0.10, 0.09, 1.0));
    text_centered(&format!("Time: {}", game.clock.label()), w / 2.0, h - BAR_H + 30.0, 20.0, INK);
    resource_strip(game);
}

fn bottom_bar(game: &mut Game, message: &mut Option<String>, dialogs: &mut VecDeque<Dialog>) -> Option<Screen> {
    let (w, h) = (screen_width(), screen_height());
    let y = h - BAR_H;
    draw_rectangle(0.0, y, w, BAR_H, Color::new(0.08, 0.10, 0.09, 1.0));
    draw_line(0.0, y, w, y, 2.0, Color::new(0.35, 0.45, 0.4, 1.0));
    let mut next = None;
    let idle = game.foe.is_none();
    if button(10.0, y + 8.0, 110.0, 40.0, "Wait 1 h", idle) || (idle && key(KeyCode::Key1)) {
        let events = game.wait(1);
        next = handle_events(game, events, message, dialogs);
    }
    if button(128.0, y + 8.0, 110.0, 40.0, "Wait 4 h", idle) || (idle && key(KeyCode::Key4)) {
        let events = game.wait(4);
        next = handle_events(game, events, message, dialogs);
    }
    // Time panel.
    let (pw, px) = (380.0, (w - 380.0) / 2.0);
    draw_rectangle(px, y + 6.0, pw, 46.0, Color::new(0.42, 0.20, 0.14, 1.0));
    draw_rectangle_lines(px, y + 6.0, pw, 46.0, 2.0, Color::new(0.6, 0.45, 0.3, 1.0));
    text_centered(&format!("Time: {}", game.clock.label()), w / 2.0, y + 25.0, 20.0, INK);
    if game.moving() {
        let left = format!("Path left: {}", duration_label(game.minutes_left() as f64));
        text_centered(&left, w / 2.0, y + 45.0, 17.0, ACCENT);
    } else {
        text_centered("time stands still", w / 2.0, y + 45.0, 16.0, DIM);
    }
    if button(w - 250.0, y + 8.0, 120.0, 40.0, "Squad", true) && next.is_none() {
        game.stop();
        *message = None;
        next = Some(Screen::Squad { selected: 0, scroll: 0, back: None });
    }
    if button(w - 122.0, y + 8.0, 112.0, 40.0, "Menu", true) && next.is_none() {
        next = Some(Screen::ScenarioSelect);
    }
    resource_strip(game);
    next
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
    let cam = Camera::looking_at(game, view.zoom, view.look.unwrap_or(game.pos));
    let on_minimap = view.minimap && minimap::outer(&game.world.map, cam.view).contains(Vec2::from(mouse_position()));
    let hovered = cam.tile_under_mouse().filter(|_| !on_minimap);
    if clicked() && !on_minimap {
        if let Some(t) = hovered {
            let target = game.world.map.nearest_passable(t, 1).filter(|_| game.world.location_covering(t).is_none()).unwrap_or(t);
            if target != game.tile() && !game.set_destination(target) {
                *message = Some("No way through.".into());
            }
            view.preview = None;
            view.look = None;
        }
    }
    if right_clicked() || key(KeyCode::Space) {
        game.stop();
    }

    // Time stands still while a window is open.
    let events = if input_blocked() { Vec::new() } else { game.tick(get_frame_time().min(0.1)) };
    let mut next = handle_events(game, events, message, dialogs);

    let cam = Camera::looking_at(game, view.zoom, view.look.unwrap_or(game.pos));
    draw_world(game, assets, &cam);
    cam.draw_fog(game);

    // Route: the one being walked, or a preview of where a click would lead.
    if game.moving() {
        draw_route(game, &game.path, game.minutes_left(), &cam, Color::new(1.0, 0.95, 0.6, 0.9));
    } else if let Some(t) = cam.tile_under_mouse().filter(|_| !on_minimap) {
        let target = game
            .world
            .location_covering(t)
            .map(|l| &game.world.locations[l])
            .filter(|l| !l.kind.is_bridge())
            .map_or(t, |l| l.tile);
        let from = game.tile();
        if view.preview.as_ref().is_none_or(|p| p.0 != target || p.1 != from) {
            let path = game.plan(target);
            let minutes = game.travel_minutes(&path);
            view.preview = Some((target, from, path, minutes));
        }
        if let Some((_, _, path, minutes)) = &view.preview {
            draw_route(game, path, *minutes, &cam, Color::new(1.0, 1.0, 1.0, 0.75));
        }
    }

    // Side panel.
    let x = screen_width() - PANEL_W;
    let panel_h = screen_height() - BAR_H;
    draw_rectangle(x, 0.0, PANEL_W, panel_h, rgb(28, 26, 24));
    let y = 12.0 + squad_panel(game, assets, x + 15.0, 12.0) + 12.0;
    if next.is_none() {
        next = location_panel(game, x + 15.0, y);
    }
    let help = ["Click the map to travel; time passes", "only while you move or wait.", "Right click / Space: stop.", "Wheel or +/-: zoom. 1 / 4: wait."];
    for (i, line) in help.iter().enumerate() {
        text(line, x + 15.0, panel_h - 80.0 + i as f32 * 18.0, 15.0, DIM);
    }

    let bar = bottom_bar(game, message, dialogs);
    next = next.or(bar);
    if minimap::toggle_button(screen_width() - 378.0, screen_height() - BAR_H + 8.0, view.minimap) {
        view.minimap = !view.minimap;
    }
    if view.minimap {
        if let Some(at) = minimap::window(game, cam.view, cam.world_rect(), surface_color) {
            view.look = Some(at);
        }
    }
    if let Some(t) = hover_tooltip(game, &cam).filter(|_| !on_minimap) {
        draw_tooltip(game, assets, &t);
    }
    if let Some(m) = message {
        let w = measure(m, 22.0).width + 40.0;
        let (cx, y) = ((screen_width() - PANEL_W) / 2.0, screen_height() - BAR_H - 50.0);
        draw_rectangle(cx - w / 2.0, y, w, 36.0, PANEL);
        text_centered(m, cx, y + 25.0, 22.0, ACCENT);
    }
    next
}
