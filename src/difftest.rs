//! Script mode for the diff test (`tools/difftest/`): a game played without a window from a
//! list of actions, with the game state written after each one, so that the same actions
//! played in the original Discord Times can be compared with Razdor step by step.
//!
//! The actions (action list v1, one JSON object per line) go through the game's own rules
//! the way the interface applies them (`ui::world_view`, `ui::mod`): a map click walks to
//! the cell with the same steps and game time a second click on the shown route gives,
//! waits play their 30-minute ticks, scenario messages queue as dialogs that `ok` or
//! `answer` closes, a pending fight opens the battle once no dialog is open, and the
//! game's generator takes the same draws the interface makes (the chord of an event, village
//! or shipyard window as it opens; the music change when a dialog closes after a won
//! battle). What the interface does in real time (the map music's timed rotation) is left
//! out: it has no fixed place in a script.
//!
//! The state (schema v1) uses the original's encodings, so the two sides compare as they
//! are: see [`State`].

use std::collections::VecDeque;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::dt::dtm::Scenario;
use crate::dt::install::DtInstall;
use crate::rules::battle::{Battle, Outcome, Team};
use crate::rules::formation::{Row, Slot};
use crate::rules::content::{Content, HeroClass};
use crate::rules::events::EventOutcome;
use crate::rules::game::{BattleResult, Event, Game, STEP_SECONDS};
use crate::rules::script::ScriptEnd;
use crate::rules::world::{Army, LocationKind, Owner, Troop};

/// One action of the list (action list v1).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Action {
    /// A new game on map file `map` (its file name, with or without `.DTm`; `demo` for the
    /// built-in demo) with hero preset `hero`: 1 knight, 2 archmage, 3 ranger.
    NewGame { map: String, hero: u8 },
    /// A click on map cell (x, y): the hero walks there (both clicks of the original).
    ClickMap { x: i32, y: i32 },
    /// Wait 1 or 4 hours.
    Wait { hours: u32 },
    /// A key: `Escape` (closes a building window), `1` / `4` (wait), `Return` / `space` (ok).
    Key { key: String },
    /// Answers the question shown.
    Answer { yes: bool },
    /// Closes the dialog shown, else the building window.
    Ok,
    /// Plays the battle shown to its end by the battle AI on both sides and closes its
    /// result box.
    BattleAuto,
    /// In battle, a press on the card at `row`, `col` of `side` (1 the player's, 2 the
    /// enemy's; rows 1 front, 2 back, 3 reserve and columns 1–6 as the original's grid
    /// numbers them): the action that cell holds for the unit whose turn it is (a strike, a
    /// shot or a spell on an enemy, a heal or a blessing on a friend, a pass on its own card),
    /// or a step to an empty own cell. The enemy's turns then play until the player's next.
    BattleAct { side: u8, row: i32, col: i32 },
    /// In battle, the space key: what a press on the acting unit's own card does (a pass,
    /// or a self-cast when its cell holds one).
    BattlePass,
    /// Nothing: only the state is written.
    Snapshot,
}

/// A unit of the hero's army.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HeroUnit {
    #[serde(rename = "type")]
    pub kind: i32,
    pub level: i32,
    pub hp: i32,
    pub xp: i32,
}

/// A unit of an AI army.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ArmyUnit {
    #[serde(rename = "type")]
    pub kind: i32,
    pub level: i32,
    pub hp: i32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HeroState {
    pub x: i32,
    pub y: i32,
    pub gold: i32,
    pub mana: i32,
    pub units: Vec<HeroUnit>,
}

/// An army record. Fields Razdor no longer has for an army it dropped are left out.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ArmyState {
    pub id: i32,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub x: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub y: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub active: Option<bool>,
    pub alive: bool,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub gold: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub units: Option<Vec<ArmyUnit>>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BuildingState {
    pub id: i32,
    pub owner: i32,
    pub gold: i32,
    pub mana: i32,
    pub goods: Vec<i32>,
}

/// The game state after a step (state schema v1), with the meanings the original's side reads
/// (`tools/difftest/memread.py`):
/// - `clock`: the calendar minute, `[0x68dcb8] div 100 + [0x68dcbc]` (world.md: game time in
///   centi-minutes since the map's start plus the start minute, the map file's start minute
///   + 1);
/// - `rng`: the state of the game's generator (engine.md §3.1);
/// - hero `x`, `y`: his cell; `gold`, `mana`: the player's;
/// - unit `type`: the map file's unit number (1-based; the record's +0 is 0-based); `level`:
///   0 for the first, as the map file and the unit record (+0x10) number it; `hp`: the
///   hit points it has (the record stores −1 for unhurt, read as its maximum), 0 dead;
///   units in record order, corpses included;
/// - army `id`: its number in the map file; `active`: on the map (+0x16a1); `alive`: not
///   destroyed (+0x16a2); `gold` (+0x16d8);
/// - building `owner` (+0x124): 0 the player, k army k, 255 none; `gold` / `mana` its
///   stocks (+0x11e, +0x160); `goods`: the items of its goods words, without the sign (a
///   negative one is the map's own) and the empty ones, in place order;
/// - `events_done`: the events that have fired at least once (event +0xa0).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct State {
    pub step: usize,
    pub map: String,
    pub clock: u64,
    pub rng: u32,
    pub hero: HeroState,
    pub armies: Vec<ArmyState>,
    pub buildings: Vec<BuildingState>,
    pub events_done: Vec<i32>,
    /// The battle on screen, if any.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub battle: Option<BattleState>,
}

/// The battle on screen (schema v1's `battle`), with the original's encodings (the battle
/// object at 0x668cf8, battle.md): `turn` the battle turn from 1 (+0xd); `actor` the unit
/// whose turn it is as `[side, row, col]` (side 1 the player's, 2 the enemy's), none once it
/// is over; `sides[0]` the player's units, `sides[1]` the enemy's, in their record order
/// (a dead unit's record is removed, the later ones move up).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BattleState {
    pub turn: u32,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub actor: Option<[i32; 3]>,
    pub sides: [Vec<BattleUnit>; 2],
}

/// A unit's record in battle: `type` 1-based (+0x23), `row` 1–3 and `col` 1–6 (+0x75,
/// +0x79), `hp` its hit points (+0x7d), `actions` the actions it has left this turn (+0x91).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BattleUnit {
    #[serde(rename = "type")]
    pub kind: i32,
    pub row: i32,
    pub col: i32,
    pub hp: i32,
    pub actions: i32,
}

/// What is on screen.
enum Screen {
    Map,
    /// The window of the building the party stands in.
    Building,
    Battle(Box<Battle>),
    /// The scenario is won or lost, or the army fell.
    Ended,
}

/// A dialog waiting to be read: an event's, the victory box or the noon report, all in the
/// original's event window, which draws a chord as it opens (0x4d15d0).
struct Dialog {
    event: bool,
    question: bool,
    cued: bool,
}

/// Where the game's content comes from.
pub enum Source<'a> {
    Demo,
    Install(&'a DtInstall),
}

/// A game played from actions.
pub struct Runner<'a> {
    source: Source<'a>,
    content: Option<Arc<Content>>,
    game: Option<Game>,
    scenario: Option<Scenario>,
    map: String,
    screen: Screen,
    last_screen_building: bool,
    dialogs: VecDeque<Dialog>,
    /// A won battle's triumph plays: closing a dialog changes the map track at once (a draw).
    triumph: bool,
    music_pick: usize,
    /// The generator draws the interface made (window chords, music changes).
    pub ui_draws: usize,
    /// What could not be applied, one line each.
    pub notes: Vec<String>,
}

/// Safety stop for a walk or a wait that does not end.
const MAX_TICKS: usize = 200_000;

impl<'a> Runner<'a> {
    pub fn new(source: Source<'a>) -> Self {
        Runner {
            source,
            content: None,
            game: None,
            scenario: None,
            map: String::new(),
            screen: Screen::Map,
            last_screen_building: false,
            dialogs: VecDeque::new(),
            triumph: false,
            music_pick: crate::rules::music::WORLD_THEME,
            ui_draws: 0,
            notes: Vec::new(),
        }
    }

    pub fn game(&self) -> Option<&Game> {
        self.game.as_ref()
    }

    /// What the replay left on screen, for a picture of it (`RAZDOR_SCENE=replay`): the game,
    /// the battle if one is open, and whether a building window is open. The dialogs waiting
    /// to be read are not handed over.
    pub fn into_view(self) -> Option<(Game, Option<Box<Battle>>, bool)> {
        let game = self.game?;
        Some(match self.screen {
            Screen::Battle(b) => (game, Some(b), false),
            Screen::Building => (game, None, true),
            Screen::Map | Screen::Ended => (game, None, false),
        })
    }

    /// Facts outside the schema (for the run log): the battle units' current stats, as the
    /// original's records hold them, and the building defence of each side.
    pub fn raw(&self) -> serde_json::Value {
        use crate::dt::data::Stat;
        let Screen::Battle(b) = &self.screen else { return serde_json::Value::Null };
        let side = |team: Team| -> Vec<serde_json::Value> {
            b.fighters
                .iter()
                .filter(|f| f.team == team && f.listed())
                .map(|f| {
                    let st = &f.stats;
                    serde_json::json!({"ab": st[Stat::AttackBlow], "as": st[Stat::AttackShot], "mp": st[Stat::MagicPower],
                        "db": st[Stat::DefenceBlow], "ds": st[Stat::DefenceShot], "maxhp": f.max_hp(), "manevres": st[Stat::Manevres],
                        "init": st[Stat::Initiative], "atk_mod": f.mods.attack, "def_mod": f.mods.defence, "init_mod": f.mods.initiative,
                        "bld_def": b.building_defence(team)})
                })
                .collect()
        };
        serde_json::json!({"battle_raw": {"sides": [side(Team::Player), side(Team::Enemy)]}})
    }

    /// Sets the game's generator (the diff test's step-local mode: each step starts from the
    /// original's state of the step before).
    pub fn set_rng(&mut self, state: u32) {
        if let Some(g) = self.game.as_mut() {
            g.rng = crate::rules::rng::Rng::new(state);
        }
    }

    fn note(&mut self, s: String) {
        self.notes.push(s);
    }

    /// Applies one action.
    pub fn apply(&mut self, action: &Action) -> Result<(), String> {
        if let Action::NewGame { map, hero } = action {
            return self.new_game(map, *hero);
        }
        if self.game.is_none() {
            return Err("no game: the list must start with new_game".into());
        }
        match action {
            Action::NewGame { .. } => unreachable!(),
            Action::ClickMap { x, y } => self.click((*x, *y)),
            Action::Wait { hours } => self.wait(*hours),
            Action::Key { key } => match key.as_str() {
                "Escape" | "Esc" => self.close_building(),
                "1" => self.wait(1),
                "4" => self.wait(4),
                "Return" | "Enter" | "space" | "Space" => self.ok(),
                other => self.note(format!("key {other}: not applied")),
            },
            Action::Answer { yes } => self.answer(*yes),
            Action::Ok => self.ok(),
            Action::BattleAuto => self.battle_auto(),
            Action::BattleAct { side, row, col } => self.battle_act(*side, *row, *col),
            Action::BattlePass => self.battle_pass(),
            Action::Snapshot => {}
        }
        self.settle();
        Ok(())
    }

    fn new_game(&mut self, map: &str, hero: u8) -> Result<(), String> {
        let class = match hero {
            1 => HeroClass::Knight,
            2 => HeroClass::Archmage,
            3 => HeroClass::Ranger,
            n => return Err(format!("hero {n}: 1, 2 or 3")),
        };
        let stem = map.trim().trim_end_matches(".DTm").trim_end_matches(".dtm");
        let mut game = match (&self.source, stem) {
            (_, "demo") | (Source::Demo, _) => {
                if stem != "demo" {
                    return Err(format!("map {map}: no install, only the demo"));
                }
                self.scenario = None;
                self.map = "demo".into();
                Game::new(Arc::new(Content::builtin()), class)
            }
            (Source::Install(dt), _) => {
                let exact = dt.maps.iter().find(|m| m.name == stem);
                let found = exact.or_else(|| {
                    let mut it = dt.maps.iter().filter(|m| m.name.starts_with(stem));
                    let first = it.next();
                    first.filter(|_| it.next().is_none())
                });
                let m = found.ok_or_else(|| format!("map {map}: not in the install (or not one map)"))?;
                let s = m.load().map_err(|e| format!("map {map}: {e}"))?;
                let content = match &self.content {
                    Some(c) => c.clone(),
                    None => {
                        let c = Arc::new(Content::from_dt(dt));
                        self.content = Some(c.clone());
                        c
                    }
                };
                let mut g = Game::from_scenario(content, &s, class);
                g.improved_ai = dt.settings.expert_ai;
                self.map = format!("{}.DTm", m.name);
                self.scenario = Some(s);
                g
            }
        };
        game.set_hero_name("");
        self.game = Some(game);
        self.screen = Screen::Map;
        self.last_screen_building = false;
        self.dialogs.clear();
        self.triumph = false;
        self.music_pick = crate::rules::music::WORLD_THEME;
        self.ui_draws = 0;
        self.settle();
        Ok(())
    }

    fn g(&mut self) -> &mut Game {
        self.game.as_mut().expect("a game")
    }

    /// The world map takes input: no dialog, no other screen.
    fn map_idle(&self) -> bool {
        self.dialogs.is_empty() && matches!(self.screen, Screen::Map)
    }

    fn close_building(&mut self) {
        if matches!(self.screen, Screen::Building) && self.dialogs.is_empty() {
            self.screen = Screen::Map;
        }
    }

    fn click(&mut self, t: (i32, i32)) {
        // A building window open: it is closed first (its Exit, as before any map click).
        self.close_building();
        if !self.map_idle() {
            self.note(format!("click_map {t:?}: the map takes no input now"));
            return;
        }
        let g = self.g();
        // A click on the building the party stands in opens it again (or its fight).
        if let Some(l) = g.location {
            let w = &g.world;
            if w.location_covering(t).or_else(|| w.location_at(t)) == Some(l) {
                let loc = &w.locations[l];
                if loc.kind == LocationKind::Camp && loc.cleared {
                    return;
                }
                if loc.defended() {
                    g.foe = Some(crate::rules::game::Foe::Garrison(l));
                } else if crate::rules::town::first_tab(loc, &g.content).is_some() {
                    self.screen = Screen::Building;
                }
                return;
            }
        }
        if !g.can_target(t) || t == g.tile() {
            self.note(format!("click_map {t:?}: not a target"));
            return;
        }
        if !g.set_destination(t) {
            self.note(format!("click_map {t:?}: no way there"));
        }
    }

    fn wait(&mut self, hours: u32) {
        self.close_building();
        if !self.map_idle() || self.g().foe.is_some() {
            self.note(format!("wait {hours}: the map takes no input now"));
            return;
        }
        self.g().begin_wait(hours);
    }

    /// Closes the front dialog; the music changes if the triumph plays.
    fn close_dialog(&mut self) {
        self.dialogs.pop_front();
        if self.triumph {
            let g = self.game.as_mut().expect("a game");
            let (pick, _) = g.music_rotate(self.music_pick);
            self.music_pick = pick;
            self.triumph = false;
            self.ui_draws += 1;
        }
    }

    /// The windows are read: the building he walked into while one opened is entered now
    /// (0x4bbc84).
    fn enter_waiting(&mut self) {
        if self.dialogs.is_empty() {
            let events = self.g().enter_waiting_building();
            self.handle(events);
        }
    }

    fn ok(&mut self) {
        match self.dialogs.front() {
            Some(d) if d.question => self.note("ok: a question is shown (answer it)".into()),
            Some(_) => {
                self.close_dialog();
                self.enter_waiting();
            }
            None if matches!(self.screen, Screen::Building) => self.screen = Screen::Map,
            None => self.note("ok: nothing to close".into()),
        }
    }

    fn answer(&mut self, yes: bool) {
        if !self.dialogs.front().is_some_and(|d| d.question) {
            self.note("answer: no question shown".into());
            return;
        }
        self.close_dialog();
        let events = self.g().answer_question(yes);
        self.handle(events);
        self.enter_waiting();
    }

    fn battle_auto(&mut self) {
        let Screen::Battle(mut b) = std::mem::replace(&mut self.screen, Screen::Map) else {
            self.note("battle_auto: no battle".into());
            return;
        };
        b.auto_play_to_end();
        self.finish_battle(&b);
    }

    /// A press on a battle card (`Action::BattleAct`), as the battle window takes it.
    fn battle_act(&mut self, side: u8, row: i32, col: i32) {
        if let Err(e) = self.press_card(side, row, col) {
            self.note(format!("battle_act {side} {row} {col}: {e}"));
        }
        self.battle_play_ai();
    }

    fn press_card(&mut self, side: u8, row: i32, col: i32) -> Result<(), &'static str> {
        let Screen::Battle(b) = &mut self.screen else { return Err("no battle") };
        let active = b.active().filter(|&a| b.fighters[a].team == Team::Player).ok_or("not the player's turn")?;
        let team = match side {
            1 => Team::Player,
            2 => Team::Enemy,
            _ => return Err("side is 1 or 2"),
        };
        let row = match row {
            1 => Row::Front,
            2 => Row::Back,
            3 => Row::Reserve,
            _ => return Err("row is 1 to 3"),
        };
        let slot = Slot::new(row, u8::try_from(col - 1).map_err(|_| "col is 1 to 6")?);
        let done = match b.at(team, slot) {
            Some(t) => match b.options(active, t).first() {
                Some(&kind) => b.act_with(t, kind).is_ok(),
                None if t == active => {
                    b.pass();
                    true
                }
                None => false,
            },
            None if team == Team::Player => b.move_active(slot).is_ok(),
            None => false,
        };
        if done {
            Ok(())
        } else {
            Err("no action on that card")
        }
    }

    /// The space key in battle: the acting unit's own-card action.
    fn battle_pass(&mut self) {
        let ok = match &mut self.screen {
            Screen::Battle(b) if b.active().is_some_and(|a| b.fighters[a].team == Team::Player) => {
                b.own_cell();
                true
            }
            _ => false,
        };
        if !ok {
            self.note("battle_pass: not the player's turn in a battle".into());
        }
        self.battle_play_ai();
    }

    /// Plays the enemy's turns until the player's next one, or the end of the battle (which
    /// is then resolved).
    fn battle_play_ai(&mut self) {
        let Screen::Battle(b) = &mut self.screen else { return };
        let game = self.game.as_mut().expect("a game");
        for _ in 0..10_000 {
            // Every action is written back into the armies (0x4c4f8c, 0x4c57bc).
            game.battle_write_back(b);
            if b.outcome() != Outcome::Ongoing {
                break;
            }
            match b.active() {
                Some(a) if b.fighters[a].team == Team::Player => return,
                Some(_) => {
                    // A plan that cannot be carried out still ends the unit's turn.
                    if b.ai_step().is_none() {
                        b.skip();
                    }
                }
                None => break,
            }
        }
        if b.outcome() == Outcome::Ongoing {
            return;
        }
        let Screen::Battle(b) = std::mem::replace(&mut self.screen, Screen::Map) else { unreachable!() };
        self.finish_battle(&b);
    }

    /// The battle's end: its result applied, the result box or the end of the game.
    fn finish_battle(&mut self, b: &Battle) {
        let g = self.game.as_mut().expect("a game");
        let result = g.resolve_battle(b);
        let won = matches!(result, BattleResult::Victory { .. });
        // The won battle's result box starts the triumph.
        self.triumph = won;
        match result {
            BattleResult::Defeat => self.screen = Screen::Ended,
            BattleResult::Victory { .. } if g.won() => self.screen = Screen::Ended,
            // The victory box is the event window: it opens with its chord (0x4d165e).
            BattleResult::Victory { .. } => self.dialogs.push_back(Dialog { event: true, question: false, cued: false }),
            BattleResult::Withdrew { .. } => {}
        }
    }

    /// The interface's handling of what happened (`world_view::handle_events`).
    fn handle(&mut self, events: Vec<Event>) {
        for e in events {
            match e {
                Event::Arrived(l) => {
                    let g = self.game.as_ref().expect("a game");
                    if g.foe.is_none() && crate::rules::town::first_tab(&g.world.locations[l], &g.content).is_some() {
                        self.screen = Screen::Building;
                    }
                }
                // The noon report is the event window too: it opens with the chord.
                Event::NewDay(_) => self.dialogs.push_back(Dialog { event: true, question: false, cued: false }),
                Event::Script(EventOutcome::Fired { message: true, .. }) => self.dialogs.push_back(Dialog { event: true, question: false, cued: false }),
                Event::Script(EventOutcome::Question(_)) => self.dialogs.push_back(Dialog { event: true, question: true, cued: false }),
                _ => {}
            }
        }
    }

    /// The draws the interface makes as windows open (`App::sounds`): a village or shipyard
    /// window, then the front dialog if it is an event's.
    fn cue(&mut self) {
        let g = self.game.as_mut().expect("a game");
        let building = matches!(self.screen, Screen::Building);
        if building && !self.last_screen_building {
            let chord = g.location.is_some_and(|l| matches!(g.world.locations[l].kind, LocationKind::Village | LocationKind::Shipyard));
            if chord {
                g.event_chord();
                self.ui_draws += 1;
            }
        }
        self.last_screen_building = building;
        if let Some(d) = self.dialogs.front_mut().filter(|d| !d.cued) {
            d.cued = true;
            if d.event {
                g.event_chord();
                self.ui_draws += 1;
            }
        }
    }

    /// Plays on until the game waits for input: events are handled, windows open, a pending
    /// fight opens its battle, a walk or a wait plays to its end.
    fn settle(&mut self) {
        for _ in 0..MAX_TICKS {
            if matches!(self.screen, Screen::Ended) {
                return;
            }
            let g = self.game.as_mut().expect("a game");
            // What the interface takes and has no place here.
            g.shown.clear();
            g.autosave_due = None;
            let _ = g.take_music_wait();
            let mut events = g.drain_events();
            let ticked = events.is_empty() && self.dialogs.is_empty() && matches!(self.screen, Screen::Map) && g.foe.is_none() && (g.moving() || g.wait_ticks > 0);
            if ticked {
                events = g.tick(STEP_SECONDS);
            }
            let any = !events.is_empty();
            self.handle(events);
            self.cue();
            // The stop's snap, after the chords of the windows it opened (0x4ad8a0).
            self.g().armies_snap();
            let g = self.game.as_mut().expect("a game");
            if self.dialogs.is_empty() {
                if matches!(g.script_end(), Some(ScriptEnd::Victory(_) | ScriptEnd::Defeat(_))) || g.army_fallen() {
                    self.screen = Screen::Ended;
                    return;
                }
                if g.foe.is_some() && matches!(self.screen, Screen::Map | Screen::Building) {
                    let mut b = g.start_battle();
                    b.begin();
                    self.screen = Screen::Battle(Box::new(b));
                    self.last_screen_building = false;
                    // The enemy's first moves, when it opens the battle.
                    self.battle_play_ai();
                    continue;
                }
            }
            if !any && !ticked {
                return;
            }
        }
        self.note("a walk or wait did not end".into());
    }

    /// The state now, as step `step`.
    pub fn state(&self, step: usize) -> Option<State> {
        let g = self.game.as_ref()?;
        let c = &g.content;
        let (x, y) = g.tile();
        let hero = HeroState { x, y, gold: g.gold, mana: g.mana, units: g.squad.iter().map(|u| HeroUnit { kind: u.def.0 as i32, level: u.level - 1, hp: u.hp.max(0), xp: u.xp }).collect() };
        let w = &g.world;
        let army = |a: &Army, active: bool, alive: bool| {
            let (x, y) = a.tile(&w.map);
            ArmyState {
                id: a.id as i32,
                x: Some(x),
                y: Some(y),
                active: Some(active),
                alive,
                gold: Some(a.gold),
                units: Some(a.troops.iter().map(|t| ArmyUnit { kind: t.unit.0 as i32, level: t.level - 1, hp: troop_hp(c, t) }).collect()),
            }
        };
        let armies = match &self.scenario {
            None => w.armies.iter().map(|a| army(a, true, true)).collect(),
            Some(s) => s
                .armies
                .iter()
                .map(|r| {
                    let id = r.id;
                    if let Some(a) = w.armies.iter().find(|a| a.id == id) {
                        army(a, true, true)
                    } else if let Some(a) = w.inactive.iter().find(|a| a.id == id) {
                        army(a, false, true)
                    } else if let Some(r) = w.respawns.iter().find(|r| r.army.id == id) {
                        army(&r.army, false, false)
                    } else {
                        ArmyState { id: id as i32, x: None, y: None, active: None, alive: false, gold: None, units: None }
                    }
                })
                .collect(),
        };
        let buildings = w
            .locations
            .iter()
            .map(|l| BuildingState {
                id: l.id as i32,
                owner: match l.owner {
                    Owner::Player => 0,
                    Owner::Army(k) => k as i32,
                    Owner::Neutral => 255,
                },
                gold: l.tribute_gold,
                mana: l.tribute_mana,
                // A market's places; ruins keep their first goods as treasure; the map load
                // wipes the goods of every other building.
                goods: match &l.shop {
                    Some(s) => s.goods().iter().map(|i| i.0 as i32).collect(),
                    None if l.kind == LocationKind::Ruins => l.map_goods.clone(),
                    None => Vec::new(),
                },
            })
            .collect();
        let events_done = match (g.script(), &self.scenario) {
            (Some(e), Some(s)) => (1..=s.events.len() as u16).filter(|&id| e.times_fired(id) > 0).map(i32::from).collect(),
            _ => Vec::new(),
        };
        let battle = match &self.screen {
            Screen::Battle(b) => Some(battle_state(b)),
            _ => None,
        };
        Some(State { step, map: self.map.clone(), clock: g.clock.total_minutes() as u64, rng: g.rng.state(), hero, armies, buildings, events_done, battle })
    }
}

/// The battle as the state shows it.
fn battle_state(b: &Battle) -> BattleState {
    let cell = |s: Slot| (s.row.number(), s.col as i32 + 1);
    let side = |team: Team| {
        b.fighters
            .iter()
            .filter(|f| f.team == team && f.listed())
            .map(|f| {
                let (row, col) = cell(f.slot);
                BattleUnit { kind: f.unit.0 as i32, row, col, hp: f.hp.max(0), actions: f.actions }
            })
            .collect()
    };
    let actor = b.active().map(|a| {
        let f = &b.fighters[a];
        let (row, col) = cell(f.slot);
        [if f.team == Team::Player { 1 } else { 2 }, row, col]
    });
    BattleState { turn: b.round, actor, sides: [side(Team::Player), side(Team::Enemy)] }
}

/// A troop's hit points: its maximum less what it lacks, 0 dead.
fn troop_hp(c: &Content, t: &Troop) -> i32 {
    if t.alive() {
        crate::rules::game::troop_unit(c, t).hp
    } else {
        0
    }
}

/// Reads an action list: one JSON object per line; blank lines and `#` comments skipped.
pub fn parse_actions(text: &str) -> Result<Vec<Action>, String> {
    text.lines()
        .enumerate()
        .filter(|(_, l)| !l.trim().is_empty() && !l.trim_start().starts_with('#'))
        .map(|(i, l)| serde_json::from_str(l).map_err(|e| format!("line {}: {e}", i + 1)))
        .collect()
}

/// Plays `actions` and returns the state after each one (step = the action's index) and the
/// notes on what could not be applied (each starts with its step).
pub fn replay(source: Source<'_>, actions: &[Action]) -> Result<(Vec<State>, Vec<String>), String> {
    let r = replay_traced(source, actions, None)?;
    Ok((r.states, r.notes))
}

/// What a replay gives.
pub struct Replay {
    pub states: Vec<State>,
    /// What could not be applied, each line starting with `step N:`.
    pub notes: Vec<String>,
    /// The generator's draws during each step (`draws[i]` for action `i`).
    pub draws: Vec<Vec<crate::rules::rng::trace::Draw>>,
    /// Facts outside the schema after each step (the battle units' stats), for the run log.
    pub raw: Vec<serde_json::Value>,
}

/// [`replay`] with the generator's draws recorded step by step. With `rng_from`, each
/// step after the first starts with the generator set to `rng_from[step − 1]` (the
/// original's states, so that each step is compared from the same draws).
pub fn replay_traced(source: Source<'_>, actions: &[Action], rng_from: Option<&[u32]>) -> Result<Replay, String> {
    use crate::rules::rng::trace;
    let mut r = Runner::new(source);
    let mut out = Replay { states: Vec::new(), notes: Vec::new(), draws: Vec::new(), raw: Vec::new() };
    trace::start();
    for (i, a) in actions.iter().enumerate() {
        if let Some(&state) = i.checked_sub(1).and_then(|k| rng_from?.get(k)) {
            r.set_rng(state);
        }
        let applied = r.apply(a).map_err(|e| format!("action {i}: {e}"));
        out.draws.push(trace::take());
        if let Err(e) = applied {
            trace::stop();
            return Err(e);
        }
        out.notes.extend(r.notes.drain(..).map(|n| format!("step {i}: {n}")));
        if let Some(s) = r.state(i) {
            out.states.push(s);
        }
        out.raw.push(r.raw());
    }
    trace::stop();
    Ok(out)
}

/// The command line: `razdor --replay <actions.jsonl> [--map <file> --hero <1|2|3>] [--out
/// <dir>]`. Returns `None` when `--replay` is not given (the game starts as usual), else the
/// exit code. `--map` puts a `new_game` before the list; the states go to
/// `<dir>/razdor.jsonl` (one line per step) or to the standard output.
pub fn cli(args: &[String]) -> Option<i32> {
    let k = args.iter().position(|a| a == "--replay")?;
    let value = |flag: &str| args.iter().position(|a| a == flag).and_then(|i| args.get(i + 1)).cloned();
    let Some(list) = args.get(k + 1) else {
        eprintln!("--replay needs an action list");
        return Some(2);
    };
    match run_cli(Path::new(list), value("--map"), value("--hero"), value("--out").map(PathBuf::from), value("--rng-from").map(PathBuf::from)) {
        Ok(()) => Some(0),
        Err(e) => {
            eprintln!("replay: {e}");
            Some(1)
        }
    }
}

/// The `rng` of each line of a state file (`original.jsonl`), by step.
fn rng_states(path: &Path) -> Result<Vec<u32>, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut out = Vec::new();
    for (i, line) in text.lines().filter(|l| !l.trim().is_empty()).enumerate() {
        let v: serde_json::Value = serde_json::from_str(line).map_err(|e| format!("{}: line {}: {e}", path.display(), i + 1))?;
        let step = v["step"].as_u64().ok_or_else(|| format!("{}: line {}: no step", path.display(), i + 1))? as usize;
        let rng = v["rng"].as_u64().ok_or_else(|| format!("{}: line {}: no rng", path.display(), i + 1))? as u32;
        if step != out.len() {
            return Err(format!("{}: line {}: step {step}, expected {}", path.display(), i + 1, out.len()));
        }
        out.push(rng);
    }
    Ok(out)
}

/// Reads the action list of the command line (`--map` puts a `new_game` first).
pub fn read_action_list(list: &Path, map: Option<String>, hero: Option<String>) -> Result<Vec<Action>, String> {
    let text = std::fs::read_to_string(list).map_err(|e| format!("{}: {e}", list.display()))?;
    let mut actions = parse_actions(&text)?;
    if let Some(map) = map {
        let hero = hero.as_deref().unwrap_or("1").parse().map_err(|_| "--hero: 1, 2 or 3".to_string())?;
        actions.insert(0, Action::NewGame { map, hero });
    }
    Ok(actions)
}

fn run_cli(list: &Path, map: Option<String>, hero: Option<String>, out: Option<PathBuf>, rng_from: Option<PathBuf>) -> Result<(), String> {
    let actions = read_action_list(list, map, hero)?;
    let rng_from = rng_from.as_deref().map(rng_states).transpose()?;
    let needs_install = actions.iter().any(|a| matches!(a, Action::NewGame { map, .. } if map.trim() != "demo"));
    let dt = if needs_install {
        crate::dt::install::load_dotenv();
        let dir = crate::dt::install::locate().ok_or("no Discord Times install (set RAZDOR_DT_DIR)")?;
        Some(DtInstall::load(&dir).map_err(|e| format!("install {}: {e}", dir.display()))?)
    } else {
        None
    };
    let source = dt.as_ref().map_or(Source::Demo, Source::Install);
    let Replay { states, notes, draws, raw } = replay_traced(source, &actions, rng_from.as_deref())?;
    for n in &notes {
        eprintln!("note: {n}");
    }
    let mut text = String::new();
    for s in &states {
        text.push_str(&serde_json::to_string(s).map_err(|e| e.to_string())?);
        text.push('\n');
    }
    match out {
        Some(dir) => {
            std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
            let path = dir.join("razdor.jsonl");
            std::fs::write(&path, text).map_err(|e| format!("{}: {e}", path.display()))?;
            // The notes and the generator's draws, step by step, for the differ.
            let mut extra = String::new();
            for (i, d) in draws.iter().enumerate() {
                let list: Vec<_> = d.iter().map(|d| serde_json::json!([d.n, d.before, format!("{}:{}", d.site.file(), d.site.line())])).collect();
                let step_notes: Vec<&String> = notes.iter().filter(|n| n.starts_with(&format!("step {i}: "))).collect();
                extra.push_str(&serde_json::json!({ "step": i, "draws": list, "notes": step_notes, "meta": raw.get(i) }).to_string());
                extra.push('\n');
            }
            let path = dir.join("razdor-run.jsonl");
            std::fs::write(&path, extra).map_err(|e| format!("{}: {e}", path.display()))?;
        }
        None => {
            let mut o = std::io::stdout().lock();
            o.write_all(text.as_bytes()).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
