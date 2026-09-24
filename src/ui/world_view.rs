//! The kingdom map: terrain, locations, roaming gangs, the party and its clock.

use macroquad::prelude::*;

use razdor::rules::battle::Team;
use razdor::rules::game::{Event, Game, Tribute};
use razdor::rules::map::{center, hex_distance, hex_neighbours, tile_at, Terrain, Tile, ROW_HEIGHT};
use razdor::rules::content::{Content, UnitId};
use razdor::rules::formation::Slot;
use razdor::rules::world::LocationKind;

use super::assets::Assets;
use super::battle_view::BattleView;
use super::screens::{message_line, squad_panel, top_bar};
use super::widgets::*;
use super::Screen;

/// Hex circumradius in pixels (pointy-top).
const HEX_R: f32 = 16.0;
/// Pixels per world unit: the distance between neighbouring hex centres.
const UNIT: f32 = HEX_R * 1.732_050_8;
const PANEL_W: f32 = 280.0;
const TOP: f32 = 44.0;

/// Stable per-tile pseudo-random number in 0..1 for decoration.
fn hash(x: i32, y: i32, salt: u32) -> f32 {
    let mut h = (x as u32).wrapping_mul(73_856_093) ^ (y as u32).wrapping_mul(19_349_663) ^ salt.wrapping_mul(83_492_791);
    h ^= h >> 13;
    h = h.wrapping_mul(0x5bd1_e995);
    h ^= h >> 15;
    (h % 10_000) as f32 / 10_000.0
}

fn rgb(r: u8, g: u8, b: u8) -> Color {
    Color::from_rgba(r, g, b, 255)
}

fn shade(c: Color, k: f32) -> Color {
    Color::new((c.r * k).min(1.0), (c.g * k).min(1.0), (c.b * k).min(1.0), c.a)
}

struct Camera {
    /// World pixel at the top-left of the map view.
    origin: Vec2,
    view: Rect,
}

impl Camera {
    fn follow(game: &Game) -> Camera {
        let view = Rect::new(0.0, TOP, screen_width() - PANEL_W, screen_height() - TOP);
        let map = &game.world.map;
        let world = vec2((map.w as f32 + 0.5) * UNIT, (map.h - 1) as f32 * ROW_HEIGHT * UNIT + 2.0 * HEX_R);
        let pad = vec2(UNIT / 2.0, HEX_R);
        let centre = Vec2::from(game.pos) * UNIT + pad;
        let mut origin = centre - vec2(view.w, view.h) / 2.0;
        origin.x = origin.x.clamp(0.0, (world.x - view.w).max(0.0));
        origin.y = origin.y.clamp(0.0, (world.y - view.h).max(0.0));
        Camera { origin: origin - pad, view }
    }

    /// Screen position of a world-space point.
    fn to_screen(&self, p: (f32, f32)) -> Vec2 {
        Vec2::from(p) * UNIT - self.origin + vec2(self.view.x, self.view.y)
    }

    fn hex_screen(&self, t: Tile) -> Vec2 {
        self.to_screen(center(t))
    }

    fn tile_under_mouse(&self) -> Option<Tile> {
        let m = Vec2::from(mouse_position());
        if !self.view.contains(m) {
            return None;
        }
        let w = (m - vec2(self.view.x, self.view.y) + self.origin) / UNIT;
        Some(tile_at((w.x, w.y)))
    }
}

fn hex(c: Vec2, color: Color) {
    // A hair larger than the circumradius so neighbours overlap without gaps.
    draw_poly(c.x, c.y, 6, HEX_R + 0.6, 30.0, color);
}

fn draw_terrain(game: &Game, cam: &Camera) {
    let map = &game.world.map;
    let top_left = cam.origin / UNIT;
    let (c0, r0) = ((top_left.x - 1.0) as i32, (top_left.y / ROW_HEIGHT - 1.0) as i32);
    let (c1, r1) = (c0 + (cam.view.w / UNIT) as i32 + 3, r0 + (cam.view.h / (UNIT * ROW_HEIGHT)) as i32 + 3);
    let is_road = |t: Tile| map.in_bounds(t) && map.terrain(t) == Terrain::Road;
    let visible = || (r0.max(0)..r1.min(map.h)).flat_map(move |y| (c0.max(0)..c1.min(map.w)).map(move |x| (x, y)));

    for (x, y) in visible() {
        let c = cam.hex_screen((x, y));
        let v = 0.93 + 0.14 * hash(x, y, 1);
        let grass = shade(rgb(64, 116, 50), v);
        let jitter = |salt| vec2(hash(x, y, salt) - 0.5, hash(x, y, salt + 50) - 0.5) * HEX_R * 1.1;
        match map.terrain((x, y)) {
            Terrain::Grass | Terrain::Road => {
                hex(c, grass);
                if hash(x, y, 2) < 0.3 {
                    let d = c + jitter(3);
                    draw_circle(d.x, d.y, 2.0, shade(grass, 1.25));
                }
            }
            Terrain::Forest => {
                hex(c, shade(rgb(40, 84, 38), v));
                for k in 0..3u32 {
                    let t = c + jitter(10 + k);
                    if hash(x, y, 30 + k) < 0.45 {
                        // Conifer.
                        draw_triangle(vec2(t.x, t.y - 9.0), vec2(t.x - 6.0, t.y + 6.0), vec2(t.x + 6.0, t.y + 6.0), rgb(26, 70, 40));
                    } else {
                        draw_circle(t.x, t.y, 6.5, shade(rgb(46, 110, 42), 0.9 + 0.2 * hash(x, y, 40 + k)));
                        draw_circle(t.x - 2.0, t.y - 2.0, 2.8, rgb(78, 140, 60));
                    }
                }
            }
            Terrain::Swamp => {
                hex(c, shade(rgb(84, 96, 58), v));
                draw_ellipse(c.x, c.y + 2.0, 8.0, 4.0, 0.0, rgb(52, 80, 70));
                draw_line(c.x - 7.0, c.y + 6.0, c.x - 7.0, c.y - 4.0, 1.5, rgb(120, 110, 60));
            }
            Terrain::Water => {
                hex(c, shade(rgb(38, 82, 140), v));
                if hash(x, y, 5) < 0.35 {
                    let wy = c.y + (hash(x, y, 6) - 0.5) * HEX_R;
                    draw_line(c.x - 5.0, wy, c.x + 5.0, wy, 1.5, rgb(90, 140, 190));
                }
            }
            Terrain::Mountain => {
                hex(c, shade(grass, 0.85));
                let top = c.y - HEX_R * 0.8;
                draw_triangle(vec2(c.x, top), vec2(c.x - HEX_R, c.y + HEX_R * 0.6), vec2(c.x + HEX_R, c.y + HEX_R * 0.6), shade(rgb(130, 128, 125), v));
                draw_triangle(vec2(c.x, top), vec2(c.x - 4.0, top + 7.0), vec2(c.x + 4.0, top + 7.0), rgb(235, 235, 240));
            }
        }
    }
    // Faint hex grid, as on the original's map.
    for (x, y) in visible() {
        let c = cam.hex_screen((x, y));
        draw_poly_lines(c.x, c.y, 6, HEX_R, 30.0, 1.0, Color::new(0.0, 0.0, 0.0, 0.12));
    }
    // Roads on top, joining the centres of neighbouring road hexes.
    let road = rgb(160, 132, 88);
    for (x, y) in visible() {
        if !is_road((x, y)) {
            continue;
        }
        let c = cam.hex_screen((x, y));
        draw_circle(c.x, c.y, HEX_R * 0.34, road);
        for n in hex_neighbours((x, y)) {
            if is_road(n) {
                let m = cam.hex_screen(n);
                draw_line(c.x, c.y, (c.x + m.x) / 2.0, (c.y + m.y) / 2.0, HEX_R * 0.62, road);
            }
        }
    }
}

fn draw_location(kind: &LocationKind, cleared: bool, c: Vec2) {
    let roof = rgb(190, 70, 50);
    let wall = rgb(210, 200, 180);
    match kind {
        LocationKind::Castle { .. } => {
            draw_rectangle(c.x - 22.0, c.y - 12.0, 44.0, 26.0, rgb(200, 196, 188));
            draw_rectangle_lines(c.x - 22.0, c.y - 12.0, 44.0, 26.0, 2.0, rgb(120, 116, 110));
            for tx in [-22.0, 14.0] {
                draw_rectangle(c.x + tx, c.y - 24.0, 9.0, 38.0, rgb(214, 210, 200));
                draw_triangle(vec2(c.x + tx + 4.5, c.y - 36.0), vec2(c.x + tx - 2.0, c.y - 24.0), vec2(c.x + tx + 11.0, c.y - 24.0), roof);
            }
            draw_rectangle(c.x - 5.0, c.y + 2.0, 10.0, 12.0, rgb(80, 60, 40));
        }
        LocationKind::Village { .. } => {
            for (dx, dy) in [(-14.0, -4.0), (4.0, -10.0), (-2.0, 8.0), (14.0, 6.0)] {
                let (x, y) = (c.x + dx, c.y + dy);
                draw_rectangle(x - 6.0, y - 4.0, 12.0, 9.0, wall);
                draw_triangle(vec2(x, y - 11.0), vec2(x - 8.0, y - 4.0), vec2(x + 8.0, y - 4.0), roof);
            }
        }
        LocationKind::Church => {
            draw_rectangle(c.x - 14.0, c.y - 6.0, 22.0, 16.0, rgb(235, 232, 225));
            draw_triangle(vec2(c.x - 3.0, c.y - 16.0), vec2(c.x - 16.0, c.y - 6.0), vec2(c.x + 10.0, c.y - 6.0), roof);
            draw_rectangle(c.x + 8.0, c.y - 22.0, 9.0, 32.0, rgb(240, 238, 232));
            draw_triangle(vec2(c.x + 12.5, c.y - 34.0), vec2(c.x + 7.0, c.y - 22.0), vec2(c.x + 18.0, c.y - 22.0), roof);
            draw_line(c.x + 12.5, c.y - 42.0, c.x + 12.5, c.y - 33.0, 2.0, ACCENT);
            draw_line(c.x + 9.0, c.y - 39.0, c.x + 16.0, c.y - 39.0, 2.0, ACCENT);
        }
        LocationKind::Camp { .. } if cleared => {
            draw_circle(c.x, c.y + 4.0, 12.0, rgb(70, 66, 60));
            draw_line(c.x - 12.0, c.y - 4.0, c.x + 10.0, c.y + 10.0, 3.0, rgb(40, 36, 32));
        }
        LocationKind::Camp { .. } => {
            for (dx, color) in [(-12.0, rgb(150, 60, 45)), (10.0, rgb(120, 90, 60))] {
                draw_triangle(vec2(c.x + dx, c.y - 16.0), vec2(c.x + dx - 12.0, c.y + 8.0), vec2(c.x + dx + 12.0, c.y + 8.0), color);
            }
            draw_circle(c.x, c.y + 12.0, 4.0, rgb(250, 160, 40));
            draw_circle(c.x, c.y + 11.0, 2.0, rgb(255, 230, 120));
        }
    }
}

fn label(s: &str, cx: f32, y: f32) {
    text_centered(s, cx + 1.0, y + 1.0, 18.0, BLACK);
    text_centered(s, cx, y, 18.0, INK);
}

/// "Gang: 2 Bandit, 1 Bandit archer".
fn gang_summary(content: &Content, enemies: &[(UnitId, Slot)]) -> String {
    let mut counts: Vec<(UnitId, usize)> = Vec::new();
    for &(id, _) in enemies {
        match counts.iter_mut().find(|(k, _)| *k == id) {
            Some((_, n)) => *n += 1,
            None => counts.push((id, 1)),
        }
    }
    let parts: Vec<String> = counts.iter().map(|(id, n)| format!("{n} {}", content.unit(*id).name)).collect();
    format!("Gang: {}", parts.join(", "))
}

fn draw_world(game: &Game, assets: &Assets, cam: &Camera) -> Option<String> {
    draw_terrain(game, cam);

    // Route.
    for &t in &game.path {
        let c = cam.hex_screen(t);
        draw_circle(c.x, c.y, 2.5, Color::new(1.0, 0.95, 0.6, 0.9));
    }

    for loc in &game.world.locations {
        let c = cam.hex_screen(loc.tile);
        draw_location(&loc.kind, loc.cleared, c);
        label(loc.name, c.x, c.y + 34.0);
    }

    let mut hover = None;
    let mouse = Vec2::from(mouse_position());
    for p in &game.world.parties {
        let c = cam.to_screen(p.pos);
        if let Some(&(leader, _)) = p.enemies.first() {
            assets.draw_unit(leader, Team::Enemy, c.x, c.y, 26.0);
        }
        if p.chasing {
            text_centered("!", c.x + 12.0, c.y - 10.0, 24.0, RED);
        }
        if (c - mouse).length() < 16.0 {
            hover = Some(gang_summary(&game.content, &p.enemies));
        }
    }

    let h = cam.to_screen(game.pos);
    draw_circle(h.x, h.y + 10.0, 12.0, Color::new(0.0, 0.0, 0.0, 0.3));
    assets.draw_unit(game.hero().def, Team::Player, h.x, h.y, 30.0);
    hover
}

/// Buttons for the location the party stands on. Returns the next screen, if any.
fn location_panel(game: &mut Game, message: &mut Option<String>, x: f32, mut y: f32) -> Option<Screen> {
    let Some(l) = game.location else {
        text("On the road.", x, y + 20.0, 20.0, DIM);
        return None;
    };
    let loc = &game.world.locations[l];
    text(loc.name, x, y + 20.0, 24.0, INK);
    y += 34.0;
    match &loc.kind {
        LocationKind::Castle { owned, .. } => {
            let what = if *owned { "Your castle. Squad healed." } else { "A lord's castle. Squad healed." };
            text(what, x, y + 14.0, 17.0, DIM);
            if button(x, y + 26.0, 240.0, 44.0, "Enter castle", true) {
                *message = None;
                return Some(Screen::Town);
            }
        }
        LocationKind::Village { .. } => {
            let tribute = game.tribute_available();
            let label = match tribute {
                Some(t) => format!("Collect tribute (+{t})"),
                None => "Tribute collected today".to_string(),
            };
            if button(x, y, 240.0, 40.0, &label, tribute.is_some()) {
                *message = game.collect_tribute().map(|t| match t {
                    Tribute::Gold(g) => format!("The village pays {g} gold."),
                    Tribute::Item(item) => format!("The village pays with a {}.", game.content.item(item).name),
                });
            }
            if button(x, y + 48.0, 240.0, 40.0, "Ask the priest to heal", tribute.is_some()) {
                game.priest_heal();
                *message = Some("The priest tends to your wounded.".into());
            }
            text("Once a day: tribute or healing.", x, y + 108.0, 17.0, DIM);
        }
        LocationKind::Church => text("You pray. The squad is healed.", x, y + 14.0, 17.0, DIM),
        LocationKind::Camp { .. } if loc.cleared => text("Only ashes remain.", x, y + 14.0, 17.0, DIM),
        LocationKind::Camp { .. } => {
            if button(x, y, 240.0, 44.0, "Attack the camp", true) {
                game.foe = Some(razdor::rules::game::Foe::Camp(l));
                return Some(Screen::Battle(Box::new(BattleView::new(game.start_battle()))));
            }
        }
    }
    None
}

fn describe(event: &Event, game: &Game) -> Option<String> {
    match event {
        Event::NewDay(r) => {
            let mut s = format!("Day {}: +{} income, -{} wages", r.day, r.income, r.wages);
            if r.unpaid > 0 {
                s += &format!(", {} unpaid refuse to fight!", r.unpaid);
            }
            Some(s)
        }
        Event::Arrived(l) => {
            let loc = &game.world.locations[*l];
            match loc.kind {
                LocationKind::Church => Some(format!("{}: the squad is healed.", loc.name)),
                _ => None,
            }
        }
        Event::Encounter(_) => Some("Bandits block your way!".into()),
    }
}

pub fn frame(game: &mut Game, assets: &Assets, message: &mut Option<String>) -> Option<Screen> {
    clear_background(rgb(20, 22, 18));

    // Input: click to walk, right click or Space to stop.
    let cam = Camera::follow(game);
    if is_mouse_button_pressed(MouseButton::Left) {
        if let Some(t) = cam.tile_under_mouse() {
            // Clicking next to a location means the location.
            let target = game
                .world
                .locations
                .iter()
                .find(|l| hex_distance(l.tile, t) <= 1)
                .map_or(t, |l| l.tile);
            if target != game.tile() && !game.set_destination(target) {
                *message = Some("No way through.".into());
            }
        }
    }
    if is_mouse_button_pressed(MouseButton::Right) || is_key_pressed(KeyCode::Space) {
        game.stop();
    }

    let mut next = None;
    for event in game.tick(get_frame_time().min(0.1)) {
        if let Some(m) = describe(&event, game) {
            *message = Some(m);
        }
        match event {
            Event::Encounter(_) => next = Some(Screen::Battle(Box::new(BattleView::new(game.start_battle())))),
            Event::Arrived(l) => match game.world.locations[l].kind {
                LocationKind::Castle { .. } => next = Some(Screen::Town),
                LocationKind::Camp { .. } if game.foe.is_some() => {
                    next = Some(Screen::Battle(Box::new(BattleView::new(game.start_battle()))))
                }
                _ => {}
            },
            Event::NewDay(_) => {}
        }
    }

    let cam = Camera::follow(game);
    let hover = draw_world(game, assets, &cam);

    // Side panel.
    let x = screen_width() - PANEL_W;
    draw_rectangle(x, TOP, PANEL_W, screen_height() - TOP, rgb(28, 26, 24));
    let y = TOP + 12.0 + squad_panel(game, assets, x + 20.0, TOP + 12.0) + 16.0;
    text(&format!("Income +{}/day", game.daily_income()), x + 20.0, y + 4.0, 18.0, ACCENT);
    text(&format!("Wages  -{}/day", game.daily_wages()), x + 20.0, y + 24.0, 18.0, DIM);
    if next.is_none() {
        next = location_panel(game, message, x + 20.0, y + 40.0);
    }
    if next.is_none() && button(x + 20.0, screen_height() - 186.0, 240.0, 40.0, "Squad & gear", true) {
        game.stop();
        *message = None;
        next = Some(Screen::Squad { selected: 0, from_town: false });
    }
    let help = [
        "Click the map to travel.",
        "Time passes only while",
        "you move. Right click or",
        "Space: stop. Red tokens",
        "are bandit gangs; '!' =",
        "they are chasing you.",
    ];
    for (i, line) in help.iter().enumerate() {
        text(line, x + 20.0, screen_height() - 130.0 + i as f32 * 19.0, 17.0, DIM);
    }

    top_bar(game);
    if let Some(h) = hover {
        let (mx, my) = mouse_position();
        let w = measure_text(&h, None, 18, 1.0).width + 16.0;
        draw_rectangle(mx + 12.0, my + 12.0, w, 26.0, PANEL);
        text(&h, mx + 20.0, my + 30.0, 18.0, INK);
    }
    message_line(message);
    next
}
