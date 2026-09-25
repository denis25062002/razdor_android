//! Modal message windows in the original's style: a title bar, a parchment text box, a row
//! of resource icons and an OK button (video notes §5: the noon report, the victory window,
//! story and quest dialogs). A scenario question has Yes and No instead of OK.

use macroquad::prelude::*;

use razdor::rules::battle::Team;
use razdor::rules::content::{ItemId, UnitId};
use razdor::rules::game::{BattleResult, DayReport, Game};

use super::assets::Assets;
use super::chrome;
use super::widgets::*;

const MARBLE_EDGE: Color = Color::new(0.38, 0.58, 0.46, 1.0);
pub const MANA: Color = Color::new(0.55, 0.72, 1.0, 1.0);

/// A resource icon with its label.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Resource {
    Gold,
    Mana,
    Income,
    Wages,
    Experience,
}

/// A picture over the text: a unit's portrait, or an event's own image.
#[derive(Clone, Debug)]
pub enum Picture {
    Unit(UnitId),
    Image(Texture2D),
}

/// How the player closed a dialog.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Close {
    Ok,
    Yes,
    No,
}

#[derive(Clone, Debug)]
pub struct Dialog {
    pub title: String,
    pub text: Vec<String>,
    /// Resource icons with a caption such as "Gold + 30".
    pub resources: Vec<(Resource, String)>,
    pub items: Vec<ItemId>,
    /// An extra line in blue (system notices).
    pub notice: Option<String>,
    /// The scenario event this dialog shows, if any.
    pub event: Option<u16>,
    /// The event's yes/no question: the dialog has Yes and No buttons.
    pub question: bool,
    pub picture: Option<Picture>,
    /// Units that joined / left the army.
    pub joined: Vec<UnitId>,
    pub left: Vec<UnitId>,
    /// Its opening sound has played.
    pub cued: bool,
}

impl Dialog {
    pub fn new(title: impl Into<String>) -> Dialog {
        Dialog {
            title: title.into(),
            text: Vec::new(),
            resources: Vec::new(),
            items: Vec::new(),
            notice: None,
            event: None,
            question: false,
            picture: None,
            joined: Vec::new(),
            left: Vec::new(),
            cued: false,
        }
    }

    /// Adds a line to the blue notice.
    pub fn add_notice(&mut self, line: &str) {
        self.notice = Some(match self.notice.take() {
            Some(n) => format!("{n}   {line}"),
            None => line.to_string(),
        });
    }

    /// The 12:00 report: gold, mana, income and wages (video notes §5).
    pub fn day_report(game: &Game, r: &DayReport) -> Dialog {
        let mut d = Dialog::new("Report on resources, income and expenses");
        d.text.push("The report shows your gold, the daily income of your castles and the wages paid to your army.".into());
        d.resources = vec![
            (Resource::Gold, format!("Gold = {}", r.gold)),
            (Resource::Mana, format!("Mana = {}", r.mana_total)),
            (Resource::Income, format!("Income + {}", r.income)),
            (Resource::Wages, format!("Wages - {}", r.wages)),
        ];
        if r.mana > 0 || r.mana_wages > 0 {
            d.text.push(format!("Mana today: + {} from your lands, - {} paid to elementals.", r.mana, r.mana_wages));
        }
        if r.unpaid > 0 {
            d.notice = Some(format!("{} unpaid units refuse to fight until they are paid.", r.unpaid));
        }
        if !r.deserted.is_empty() {
            let names: Vec<&str> = r.deserted.iter().map(|&u| game.content.unit(u).name.as_str()).collect();
            d.text.push(format!("Left the army unpaid: {}.", names.join(", ")));
        }
        d
    }

    /// A castle or fort taken without a fight.
    pub fn captured(game: &Game, l: usize) -> Dialog {
        let loc = &game.world.locations[l];
        let mut d = Dialog::new("A new stronghold");
        d.text.push(format!("Nobody defends {}. You take it: it pays you {} gold a day from now on.", loc.name, loc.gold_income));
        d.resources.push((Resource::Income, format!("Income + {}", game.daily_income())));
        d
    }

    /// The window after a won battle: gold and mana taken, a captured building, the loot.
    pub fn victory(game: &Game, result: &BattleResult) -> Option<Dialog> {
        let BattleResult::Victory { reward, mana, lost, loot, left_behind, level_ups, captured } = result else {
            return None;
        };
        let mut d = Dialog::new("Victory over the enemy!");
        if let Some(l) = captured {
            let loc = &game.world.locations[*l];
            d.text.push(format!("You have taken {}. It pays you {} gold a day from now on.", loc.name, loc.gold_income));
        }
        if *reward > 0 {
            d.text.push("In this battle you won gold from the enemy.".into());
        }
        if *mana > 0 {
            d.text.push("Grateful for your mercy, the surrendered troops pray for you.".into());
        }
        if *lost > 0 {
            d.text.push(format!("{lost} of your units fell. Their bodies can be raised in a town or church within a week."));
        }
        for &(i, level) in level_ups {
            d.text.push(format!("{} reaches level {level}.", game.squad[i].name(&game.content)));
        }
        if d.text.is_empty() {
            d.text.push("The enemy is beaten.".into());
        }
        d.resources.push((Resource::Gold, format!("Gold + {reward}")));
        if *mana > 0 {
            d.resources.push((Resource::Mana, format!("Mana + {mana}")));
        }
        if captured.is_some() {
            d.resources.push((Resource::Income, format!("Income + {}", game.daily_income())));
        }
        d.items = loot.clone();
        if *left_behind > 0 {
            d.notice = Some(format!("{left_behind} items were left behind: the pack is full."));
        }
        Some(d)
    }
}

/// Small drawn icons for the resources (our own shapes).
pub fn resource_icon(r: Resource, cx: f32, cy: f32, s: f32) {
    // The original's pictures: coins, the magic book, the house, the paid knight.
    let art = match r {
        Resource::Gold => Some(0),
        Resource::Mana => Some(1),
        Resource::Income => Some(2),
        Resource::Wages => Some(3),
        Resource::Experience => None,
    };
    if let Some(n) = art {
        let name = if s <= 48.0 { format!("res{n}-44") } else { format!("res{n}") };
        if let Some(t) = super::chrome::win(&name) {
            super::chrome::tex(&t, Rect::new(cx - s / 2.0, cy - s / 2.0, s, s), WHITE);
            return;
        }
    }
    match r {
        Resource::Gold => {
            for k in 0..3 {
                let y = cy + s * 0.25 - k as f32 * s * 0.18;
                draw_ellipse(cx, y, s * 0.36, s * 0.14, 0.0, Color::new(0.85, 0.66, 0.2, 1.0));
                draw_ellipse_lines(cx, y, s * 0.36, s * 0.14, 0.0, 1.5, Color::new(0.5, 0.35, 0.1, 1.0));
            }
        }
        Resource::Mana => {
            let (t, b) = (vec2(cx, cy - s * 0.45), vec2(cx, cy + s * 0.45));
            let (l, r) = (vec2(cx - s * 0.28, cy), vec2(cx + s * 0.28, cy));
            draw_triangle(t, l, r, MANA);
            draw_triangle(b, l, r, Color::new(0.3, 0.5, 0.9, 1.0));
        }
        Resource::Income => {
            draw_rectangle(cx - s * 0.3, cy - s * 0.05, s * 0.6, s * 0.4, Color::new(0.8, 0.7, 0.55, 1.0));
            draw_triangle(vec2(cx, cy - s * 0.42), vec2(cx - s * 0.4, cy - s * 0.05), vec2(cx + s * 0.4, cy - s * 0.05), Color::new(0.75, 0.3, 0.2, 1.0));
        }
        Resource::Wages => {
            draw_circle(cx, cy - s * 0.2, s * 0.16, Color::new(0.75, 0.75, 0.8, 1.0));
            draw_rectangle(cx - s * 0.22, cy - s * 0.04, s * 0.44, s * 0.46, Color::new(0.6, 0.62, 0.7, 1.0));
        }
        Resource::Experience => {
            let star = Color::new(0.95, 0.85, 0.35, 1.0);
            let (r, k) = (s * 0.42, s * 0.18);
            for i in 0..5 {
                let a = std::f32::consts::TAU * i as f32 / 5.0 - std::f32::consts::FRAC_PI_2;
                let (b, c) = (a - 0.63, a + 0.63);
                draw_triangle(
                    vec2(cx + r * a.cos(), cy + r * a.sin()),
                    vec2(cx + k * b.cos(), cy + k * b.sin()),
                    vec2(cx + k * c.cos(), cy + k * c.sin()),
                    star,
                );
            }
            draw_circle(cx, cy, k, star);
        }
    }
}

/// A row of unit portraits with a caption. Returns its height.
fn unit_row(assets: &Assets, label: &str, units: &[UnitId], x: f32, y: f32) -> f32 {
    if units.is_empty() {
        return 0.0;
    }
    text(label, x + 30.0, y + 30.0, 18.0, INK);
    let lx = x + 40.0 + measure(label, 18.0).width;
    for (k, &u) in units.iter().enumerate().take(8) {
        assets.draw_unit(u, Team::Player, lx + 26.0 + k as f32 * 56.0, y + 26.0, 50.0);
    }
    60.0
}

/// Draws `d` centred on the screen; returns how it was closed: OK (or Enter, Escape), or for
/// a question Yes (Enter) or No (Escape).
pub fn draw(d: &Dialog, assets: &Assets) -> Option<Close> {
    let (sw, sh) = (screen_width(), screen_height());
    draw_rectangle(0.0, 0.0, sw, sh, Color::new(0.0, 0.0, 0.0, 0.35));
    // A long story text widens the window rather than running off the screen.
    let fit = |w: f32| -> (f32, Vec<String>) { (w, d.text.iter().flat_map(|t| wrap(t, w - 80.0, 19.0)).collect()) };
    let (mut w, mut lines) = fit(620.0f32.min(sw - 20.0));
    if lines.len() as f32 * 23.0 > sh * 0.45 {
        (w, lines) = fit(980.0f32.min(sw - 20.0));
    }
    let text_h = lines.len() as f32 * 23.0 + 24.0;
    let res_h = if d.resources.is_empty() { 0.0 } else { 104.0 };
    let items_h = if d.items.is_empty() { 0.0 } else { 60.0 };
    let notice_h = if d.notice.is_some() { 26.0 } else { 0.0 };
    let pic_h = if d.picture.is_some() { 140.0 } else { 0.0 };
    let units_h = [&d.joined, &d.left].iter().filter(|u| !u.is_empty()).count() as f32 * 60.0;
    let h = 34.0 + 16.0 + pic_h + text_h + res_h + items_h + units_h + notice_h + 64.0;
    let (x, y) = ((sw - w) / 2.0, ((sh - h) / 2.0).max(10.0));
    chrome::window(Rect::new(x, y, w, h), &d.title, chrome::Skin::Marble, false);
    let mut cy = y + 44.0;
    match &d.picture {
        Some(Picture::Unit(u)) => {
            let r = Rect::new(x + w / 2.0 - 64.0, cy, 128.0, 128.0);
            assets.draw_portrait(*u, Team::Player, r);
            chrome::silver_frame(r, 1.5);
        }
        Some(Picture::Image(tex)) => {
            let k = (128.0 / tex.height()).min((w - 60.0) / tex.width());
            let (tw, th) = (tex.width() * k, tex.height() * k);
            draw_texture_ex(tex, x + (w - tw) / 2.0, cy, WHITE, DrawTextureParams { dest_size: Some(vec2(tw, th)), ..Default::default() });
            draw_rectangle_lines(x + (w - tw) / 2.0, cy, tw, th, 2.0, MARBLE_EDGE);
        }
        None => {}
    }
    cy += pic_h;
    chrome::text_box(Rect::new(x + 16.0, cy, w - 32.0, text_h));
    for (i, line) in lines.iter().enumerate() {
        chrome::shadow_centered(line, x + w / 2.0, cy + 30.0 + i as f32 * 23.0, 19.0, Color::new(1.0, 0.9, 0.66, 1.0));
    }
    cy += text_h + 10.0;
    if !d.resources.is_empty() {
        let n = d.resources.len() as f32;
        let step = (w - 60.0) / n;
        for (k, (r, label)) in d.resources.iter().enumerate() {
            let cx = x + 30.0 + step * (k as f32 + 0.5);
            let c = if *r == Resource::Mana { MANA } else { Color::new(0.7, 0.92, 0.75, 1.0) };
            chrome::shadow_centered(label, cx, cy + 18.0, 18.0, c);
            resource_icon(*r, cx, cy + 62.0, 70.0);
        }
        cy += res_h;
    }
    if !d.items.is_empty() {
        text("Items found:", x + 30.0, cy + 30.0, 18.0, INK);
        for (k, &item) in d.items.iter().enumerate().take(8) {
            assets.draw_item(item, x + 150.0 + k as f32 * 54.0, cy + 4.0, 48.0);
        }
        cy += items_h;
    }
    cy += unit_row(assets, "Joined the army:", &d.joined, x, cy);
    cy += unit_row(assets, "Left the army:", &d.left, x, cy);
    if let Some(n) = &d.notice {
        chrome::shadow_centered(n, x + w / 2.0, cy + 18.0, 18.0, Color::new(0.3, 0.72, 1.0, 1.0));
    }
    let by = y + h - 52.0;
    // The silver line over the buttons.
    draw_line(x + 2.0, by - 10.0, x + w - 2.0, by - 10.0, 1.5, chrome::SILVER);
    if d.question {
        let yes = button(x + w / 2.0 - 140.0, by, 120.0, 38.0, "Yes", true) || key(KeyCode::Enter) || key(KeyCode::Y);
        let no = button(x + w / 2.0 + 20.0, by, 120.0, 38.0, "No", true) || key(KeyCode::Escape) || key(KeyCode::N);
        return if yes {
            Some(Close::Yes)
        } else if no {
            Some(Close::No)
        } else {
            None
        };
    }
    let ok = button(x + w / 2.0 - 60.0, by, 120.0, 38.0, "OK", true);
    (ok || key(KeyCode::Enter) || key(KeyCode::Escape)).then_some(Close::Ok)
}
