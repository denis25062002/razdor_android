//! Property panels of the selected building, army or point. Each works on a copy of the
//! record and returns the command that stores it (merged into one undo step per field).

use macroquad::prelude::*;

use razdor::dt::dtm::{Army, Building, Point, Scenario};
use razdor::editor::palette::{self, building_type_label, Names, Palette};
use razdor::editor::records;
use razdor::editor::Command;
use razdor::rules::content::Content;

use super::form::*;
use crate::ui::widgets::*;

/// Panel state kept between frames.
#[derive(Default)]
pub struct PanelState {
    pub tab: usize,
    pub scroll: f32,
    /// What the panel showed last (a new selection starts at the top).
    shown: Option<String>,
}

impl PanelState {
    fn show(&mut self, what: &str) {
        if self.shown.as_deref() != Some(what) {
            self.shown = Some(what.to_string());
            self.scroll = 0.0;
        }
    }
}

/// What the panels need to know about the install.
pub struct Ctx<'a> {
    pub names: &'a Names,
    pub palette: &'a Palette,
    pub content: Option<&'a Content>,
}

/// The panel's frame and title; returns the area under the title and tabs for the form.
fn frame(rect: Rect, title: &str, tabs_labels: &[&str], state: &mut PanelState) -> Rect {
    draw_rectangle(rect.x, rect.y, rect.w, rect.h, Color::new(0.1, 0.095, 0.09, 0.96));
    draw_rectangle_lines(rect.x, rect.y, rect.w, rect.h, 1.0, DIM);
    let mut t = title.to_string();
    while measure(&t, 20.0).width > rect.w - 20.0 && !t.is_empty() {
        t.pop();
    }
    text(&t, rect.x + 10.0, rect.y + 24.0, 20.0, ACCENT);
    let mut y = rect.y + 34.0;
    if !tabs_labels.is_empty() {
        let before = state.tab;
        y += tabs(rect.x + 6.0, y, rect.w - 12.0, tabs_labels, &mut state.tab);
        if state.tab != before {
            state.scroll = 0.0;
        }
    }
    Rect::new(rect.x + 10.0, y + 4.0, rect.w - 20.0, rect.bottom() - y - 50.0)
}

/// Mouse wheel over the form scrolls it.
fn scroll(state: &mut PanelState, area: Rect, content: f32) {
    if mouse_in(area.x, area.y, area.w, area.h) && !popup_open() {
        let wh = wheel();
        if wh != 0.0 {
            state.scroll -= wh * 40.0;
        }
    }
    state.scroll = state.scroll.clamp(0.0, (content - area.h + 20.0).max(0.0));
}

/// The footer buttons: returns true for "Delete".
fn footer(rect: Rect, what: &str) -> bool {
    small_button(rect.x + 10.0, rect.bottom() - 40.0, 150.0, 30.0, &format!("Delete {what}"), true)
}

fn changed<T: PartialEq>(before: &T, after: &T, form: &Form) -> Option<String> {
    (before != after).then(|| form.changed.clone().unwrap_or_else(|| "edit".into()))
}

fn owner_options(s: &Scenario) -> Options {
    let mut o = vec![(0xFF, "(none: the neutral owner)".to_string())];
    o.extend((1..=s.armies.len().min(254) as u8).map(|id| (id as i64, army_label(s, id))));
    o
}

pub fn building_panel(state: &mut PanelState, s: &Scenario, id: u16, ctx: &Ctx, rect: Rect) -> Option<(Command, String)> {
    let orig = s.building(id)?;
    let mut b: Building = orig.clone();
    state.show(&format!("b{id}"));
    let title = format!("{} #{id}  {}", building_type_label(b.kind), b.name.trim());
    let area = frame(rect, &title, &["General", "Troops", "Trade", "Faction", "Events"], state);
    let mut f = Form::new(&format!("b{id}"), area, state.scroll);
    let n = ctx.names;
    let (w, h) = (s.width() as i64, s.height() as i64);
    match state.tab {
        0 => {
            f.text("name", "Name", &mut b.name);
            f.text("owner_name", "Neutral owner", &mut b.owner_name);
            f.memo("description", "Description", &mut b.description, 4);
            let kinds = list_options(&(0..16).map(building_type_label).collect::<Vec<_>>(), 0);
            f.pick("kind", "Type", &mut b.kind, &kinds);
            let variants: Options = ctx
                .palette
                .pictures_of(b.picture_type)
                .map(|p| (p.variant as i64, format!("Picture {} ({}x{})", p.variant, p.size.0, p.size.1)))
                .collect();
            let before = b.picture_variant;
            if variants.is_empty() {
                f.num("variant", "Picture", &mut b.picture_variant, 0, 255);
            } else {
                f.pick("variant", "Picture", &mut b.picture_variant, &variants);
            }
            if b.picture_variant != before {
                // A new picture brings its footprint.
                (b.size_x, b.size_y) = ctx.palette.footprint(b.picture_type, b.picture_variant);
            }
            f.num("picture_type", "Picture type", &mut b.picture_type, 0, 15);
            f.num("x", "X (bottom-right)", &mut b.x, 0, w - 1);
            f.num("y", "Y (bottom-right)", &mut b.y, 0, h - 1);
            f.num("size_x", "Footprint width", &mut b.size_x, 1, 12);
            f.num("size_y", "Footprint height", &mut b.size_y, 1, 12);
            f.pick("owner", "Owner army", &mut b.owner_army, &owner_options(s));
            f.pick("linked", "Linked building", &mut b.linked_building, &building_options(s));
            f.note("A village's castle; a dungeon's other end.", DIM);
            f.heading("Starts as the player's for");
            for (k, class) in palette::HERO_CLASSES.iter().enumerate() {
                f.flag(&format!("start{k}"), class, &mut b.start_for[k]);
            }
        }
        1 => {
            let units = unit_options(n);
            f.heading("Barracks");
            f.flag("has_barracks", "Has barracks", &mut b.has_barracks);
            f.flag("all_types", "Recruits all types (bandits too)", &mut b.recruit_all_types);
            f.slot_header("Unit", "At start", "Most");
            for (i, r) in b.barracks.iter_mut().enumerate() {
                let (mut u, mut a, mut m) = (r.unit, r.start_count, r.max_count);
                f.slot(&format!("bar{i}"), &units, &mut u, &mut a, 9, &mut m, 9);
                if (u, a, m) != (r.unit, r.start_count, r.max_count) {
                    *r = if u == 0 { Default::default() } else { razdor::dt::dtm::RecruitSlot { unit: u, start_count: a, max_count: m } };
                }
            }
            f.heading("Garrison");
            f.troops("gar", &units, &mut b.garrison);
            f.num("defence", "Extra defence (%)", &mut b.garrison_extra_defence, 0, 255);
            f.flag("ai_only", "Garrison serves the AI only", &mut b.garrison_ai_only);
        }
        2 => {
            let arts = artefact_options(n);
            let is_ruin = b.kind == 12;
            f.heading(if is_ruin { "Treasure (artefacts)" } else { "Goods always for sale" });
            for k in 0..records::GOODS {
                let mut a = b.artifact_slots[k];
                f.pick(&format!("goods{k}"), &format!("Slot {}", k + 1), &mut a, &arts);
                if a != b.artifact_slots[k] {
                    records::set_goods(&mut b, k, a);
                }
            }
            if is_ruin {
                f.num("treasure", "Treasure gold", &mut b.price_max, 0, 50_000);
            } else {
                f.heading("Random goods");
                f.num("random", "How many", &mut b.random_artifacts_for_sale, 0, 12);
                f.num("price_min", "Lowest price", &mut b.price_min, 0, 50_000);
                f.num("price_max", "Highest price", &mut b.price_max, 0, 50_000);
            }
            f.heading("Spells to learn");
            let spells = spell_options(n);
            for k in 0..6 {
                f.pick(&format!("spell{k}"), &format!("Spell {}", k + 1), &mut b.spells_for_sale[k], &spells);
            }
        }
        3 => {
            let factions = list_options(&palette::FACTIONS, 1);
            f.pick("faction", "Faction", &mut b.faction, &factions);
            if f.button("Attitudes from the faction's row", true) {
                if let Some(row) = s.header.relations.get((b.faction as usize).wrapping_sub(1)) {
                    b.relations = *row;
                    f.changed = Some("relations".into());
                }
            }
            f.heading("Attitude towards");
            f.relations("rel", &mut b.relations);
            f.heading("Gold");
            f.num("gold", "Income per day", &mut b.gold_per_day, 0, 2500);
            f.num("gold_max", "Most kept (villages)", &mut b.gold_max, 0, 25_000);
            f.heading("Mana");
            f.num("mana", "Income per day", &mut b.mana_per_day, 0, 255);
            f.num("mana_max", "Most kept", &mut b.mana_max, 0, 255);
        }
        _ => {
            f.heading("Local events");
            f.note("Local events, quests and rumours checked here, in this order (edit them with the Events button).", DIM);
            let used = records::used_events(&b.event_slots, b.event_count).to_vec();
            match f.event_list("events", &used, &event_options(s)) {
                EventListEdit::Add(e) => {
                    records::add_event(&mut b.event_slots, &mut b.event_count, e);
                    f.changed = Some("events".into());
                }
                EventListEdit::Remove(i) => {
                    records::remove_event(&mut b.event_slots, &mut b.event_count, i);
                    f.changed = Some("events".into());
                }
                EventListEdit::None => {}
            }
        }
    }
    let content_h = f.content_height();
    scroll(state, area, content_h);
    if footer(rect, "building") {
        return Some((Command::DeleteBuilding { id }, String::new()));
    }
    let key = changed(orig, &b, &f)?;
    Some((Command::SetBuilding { id, building: Box::new(b) }, key))
}

pub fn army_panel(state: &mut PanelState, s: &Scenario, id: u8, ctx: &Ctx, rect: Rect) -> Option<(Command, String)> {
    let orig = s.army(id)?;
    let mut a: Army = orig.clone();
    state.show(&format!("a{id}"));
    let area = frame(rect, &format!("Army {}", army_label(s, id)), &["Leader", "Troops", "AI", "Faction"], state);
    let mut f = Form::new(&format!("a{id}"), area, state.scroll);
    let n = ctx.names;
    let (w, h) = (s.width() as i64, s.height() as i64);
    match state.tab {
        0 => {
            f.text("name", "Army name", &mut a.name);
            f.text("leader_name", "Leader's name", &mut a.leader_name);
            let units = unit_options(n);
            f.pick("leader", "Leader", &mut a.leader_unit, &units);
            let mut lv = a.leader_level + 1;
            f.num("leader_level", "Leader's level", &mut lv, 1, 10);
            a.leader_level = lv - 1;
            f.pick("named", "Named character", &mut a.named_character, &named_options(s));
            f.pick("home", "Home building", &mut a.home_building, &building_options(s));
            let models = list_options(&palette::ARMY_MODELS.iter().map(|m| m.1).collect::<Vec<_>>(), 1);
            f.pick("model", "Map figure", &mut a.model, &models);
            f.pick("ship", "Ship", &mut a.ship, &list_options(&palette::SHIPS, 0));
            f.num("x", "X", &mut a.x, 0, w - 1);
            f.num("y", "Y", &mut a.y, 0, h - 1);
            f.heading("Carries");
            let arts = artefact_options(n);
            for k in 0..3 {
                f.pick(&format!("art{k}"), &format!("Artefact {}", k + 1), &mut a.artifacts[k], &arts);
            }
            f.pick("spell", "Spell on the army", &mut a.spell, &spell_options(n));
        }
        1 => {
            f.memo("description", "Description", &mut a.description, 3);
            f.heading("Troops");
            f.troops("t", &unit_options(n), &mut a.troops);
            if let Some(c) = ctx.content {
                f.note(&format!("Strength (tactical cost): {}", records::army_strength(&a, c)), INK);
            }
            f.note(&format!("Stored editor values: {} / {}", a.tactical_cost_1, a.tactical_cost_2), DIM);
            f.num("gold", "Starting gold", &mut a.gold_income, 0, 65_535);
            f.heading("Hiring and garrison");
            f.num("hire_xp", "Experience for hired units", &mut a.hire_bonus_exp, 0, 65_535);
            f.flag("xp_like", "Hired units get the player's experience", &mut a.exp_like_player);
            f.num("garrison", "Garrison strength", &mut a.garrison_strength, 0, 255);
        }
        2 => {
            let behaviours = list_options(&palette::BEHAVIOURS, 0);
            f.pick("behaviour", "Behaviour", &mut a.behaviour, &behaviours);
            f.pick("target", "Target choice", &mut a.target_model, &list_options(&palette::TARGET_MODELS, 0));
            f.num("aggression", "Aggression", &mut a.aggression, -128, 127);
            f.heading("Movement");
            f.flag("inactive", "Inactive at the start", &mut a.inactive);
            f.flag("patrols", "Patrols", &mut a.patrols);
            f.num("radius", "Patrol radius", &mut a.patrol_radius, 0, 255);
            f.num("speed", "Speed correction", &mut a.speed_correction, -10, 10);
            f.heading("Targets");
            f.flag("ignored", "Ignored by the AI", &mut a.ignored_by_ai);
            f.flag("hunts", "Hunts only the player", &mut a.hunts_player_only);
            f.flag("no_random", "No random targets", &mut a.no_random_targets);
            f.flag("no_social", "Does not meet other armies", &mut a.no_socialising);
            f.flag("no_buildings", "No interest in buildings", &mut a.no_building_interest);
            f.heading("Respawn and spoils");
            f.num("respawn", "Respawn after (days)", &mut a.respawn_days, 0, 255);
            f.flag("respawn_all", "Respawn the whole army", &mut a.respawn_all);
            f.num("exp", "Experience correction (%)", &mut a.exp_correction, 0, 255);
            f.flag("no_money", "Units carry no money", &mut a.no_money);
        }
        _ => {
            f.pick("faction", "Faction", &mut a.faction, &list_options(&palette::FACTIONS, 1));
            if f.button("Attitudes from the faction's row", true) {
                if let Some(row) = s.header.relations.get((a.faction as usize).wrapping_sub(1)) {
                    a.relations = *row;
                    f.changed = Some("relations".into());
                }
            }
            f.heading("Attitude towards");
            f.relations("rel", &mut a.relations);
        }
    }
    let content_h = f.content_height();
    scroll(state, area, content_h);
    if footer(rect, "army") {
        return Some((Command::DeleteArmy { id }, String::new()));
    }
    let key = changed(orig, &a, &f)?;
    Some((Command::SetArmy { id, army: Box::new(a) }, key))
}

pub fn point_panel(state: &mut PanelState, s: &Scenario, id: u8, rect: Rect) -> Option<(Command, String)> {
    let orig = s.points.get((id as usize).checked_sub(1)?)?;
    let mut p: Point = orig.clone();
    state.show(&format!("p{id}"));
    let kind = if p.model == 8 { "Lantern" } else { "Event point" };
    let area = frame(rect, &format!("{kind} #{id}"), &[], state);
    let mut f = Form::new(&format!("p{id}"), area, state.scroll);
    let mut lantern = (p.model == 8) as u8;
    f.flag("lantern", "Lantern (reveals the area around it)", &mut lantern);
    p.model = if lantern != 0 { 8 } else { 9 };
    f.flag("active", "Active at the start", &mut p.active);
    f.num("radius", "Radius at the start", &mut p.radius, 0, 24);
    f.num("x", "X", &mut p.x, 0, s.width() as i64 - 1);
    f.num("y", "Y", &mut p.y, 0, s.height() as i64 - 1);
    f.heading("Attached events");
    f.note("Up to 5, as in the original editor.", DIM);
    let used = records::used_events(&p.event_slots, p.event_count).to_vec();
    let mut slots5 = [0u16; 5];
    slots5.copy_from_slice(&p.event_slots[..5]);
    match f.event_list("events", &used, &event_options(s)) {
        EventListEdit::Add(e) => {
            let mut n = p.event_count.min(5);
            if records::add_event(&mut slots5, &mut n, e) {
                p.event_slots[..5].copy_from_slice(&slots5);
                p.event_count = n;
                f.changed = Some("events".into());
            }
        }
        EventListEdit::Remove(i) => {
            records::remove_event(&mut p.event_slots, &mut p.event_count, i);
            f.changed = Some("events".into());
        }
        EventListEdit::None => {}
    }
    let content_h = f.content_height();
    scroll(state, area, content_h);
    if footer(rect, "point") {
        return Some((Command::DeletePoint { id }, String::new()));
    }
    let key = changed(orig, &p, &f)?;
    Some((Command::SetPoint { id, point: Box::new(p) }, key))
}
