//! macroquad presentation layer. Reads rules state and calls rules methods.
pub mod assets;
pub mod battle_view;
pub mod building_view;
pub mod dialog;
pub mod dt_art;
pub mod items_view;
pub mod minimap;
pub mod screens;
pub mod story;
pub mod widgets;
pub mod world_view;

use std::collections::VecDeque;
use std::sync::Arc;

use razdor::dt::dtm::Scenario;
use razdor::rules::content::Content;
use razdor::rules::game::Game;
use razdor::rules::script::ScriptEnd;

use assets::Assets;
use battle_view::BattleView;
use building_view::BuildingView;
use dialog::{Close, Dialog};
use world_view::MapView;

pub enum Screen {
    /// The built-in demo or a map of the install.
    ScenarioSelect,
    /// Hero class for the demo (`None`) or for `scenarios[i]`.
    ClassSelect { scenario: Option<usize> },
    WorldMap,
    /// The window of the building the hero stands in.
    Building(BuildingView),
    /// Hero and army screen: selected squad member, backpack scroll, and the building window
    /// "Back" returns to (the map if none).
    Squad { selected: usize, scroll: usize, back: Option<BuildingView> },
    Battle(Box<BattleView>),
    /// The quest journal, with the selected line.
    Journal { selected: usize },
    GameOver,
    Victory,
}

/// A scenario of the player's install, loaded for the select screen.
pub struct ScenarioEntry {
    /// File name without the extension.
    pub file: String,
    pub scenario: Scenario,
}

pub struct App {
    pub assets: Assets,
    /// The built-in demo content.
    pub demo: Arc<Content>,
    /// Content of the player's install, if present.
    pub dt_content: Option<Arc<Content>>,
    /// Maps of the install (read at start, never written).
    pub scenarios: Vec<ScenarioEntry>,
    pub game: Option<Game>,
    pub screen: Screen,
    /// One-line notice shown on the world map / town screens.
    pub message: Option<String>,
    /// Modal windows waiting to be read (noon reports, victories), first on top.
    pub dialogs: VecDeque<Dialog>,
    pub map_view: MapView,
}

impl App {
    pub fn new(assets: Assets, demo: Arc<Content>) -> Self {
        let dt_content = assets.dt.as_ref().map(|d| Arc::new(Content::from_dt(&d.install)));
        let scenarios = assets
            .dt
            .iter()
            .flat_map(|d| d.install.maps.iter())
            .filter_map(|m| match m.load() {
                Ok(scenario) => Some(ScenarioEntry { file: m.name.clone(), scenario }),
                Err(e) => {
                    eprintln!("{}: {e}", m.name);
                    None
                }
            })
            .collect();
        App {
            assets,
            demo,
            dt_content,
            scenarios,
            game: None,
            screen: Screen::ScenarioSelect,
            message: None,
            dialogs: VecDeque::new(),
            map_view: MapView::default(),
        }
    }

    pub fn frame(&mut self) {
        // A dialog on top: the screen below is drawn but takes no input.
        widgets::set_input_blocked(!self.dialogs.is_empty());
        let mut next = match (&mut self.screen, &mut self.game) {
            (Screen::ScenarioSelect, _) => screens::scenario_select(&self.scenarios, self.dt_content.is_some()),
            (Screen::ClassSelect { scenario }, game) => {
                let pick = scenario.and_then(|i| Some((self.scenarios.get(i)?, self.dt_content.clone()?)));
                screens::class_select(game, &self.demo, pick, &self.assets)
            }
            (Screen::WorldMap, Some(game)) => {
                world_view::frame(game, &self.assets, &mut self.map_view, &mut self.message, &mut self.dialogs)
            }
            (Screen::Building(view), Some(game)) => {
                building_view::frame(game, &self.assets, view, &mut self.message, &mut self.dialogs)
            }
            (Screen::Squad { selected, scroll, back }, Some(game)) => {
                items_view::squad(game, &self.assets, selected, scroll, back, &mut self.message)
            }
            (Screen::Battle(view), Some(game)) => view.frame(game, &self.assets, &mut self.message, &mut self.dialogs),
            (Screen::Journal { selected }, Some(game)) => story::journal(game, &self.assets, selected),
            (Screen::GameOver, game) => screens::game_over(game),
            (Screen::Victory, game) => screens::victory(game),
            (_, None) => Some(Screen::ScenarioSelect),
        };
        widgets::set_input_blocked(false);
        if let Some(d) = self.dialogs.front() {
            if let Some(close) = dialog::draw(d, &self.assets) {
                let asked = self.dialogs.pop_front().is_some_and(|d| d.question);
                // A scenario question: the answer goes to the event engine.
                if let (true, Some(game)) = (asked, self.game.as_mut()) {
                    let events = game.answer_question(close == Close::Yes);
                    let after = world_view::handle_events(game, events, &mut self.message, &mut self.dialogs);
                    next = next.or(after);
                }
            }
        }
        // A victory or defeat event ends the game once its window is read.
        if next.is_none() && self.dialogs.is_empty() {
            let end = self.game.as_ref().and_then(Game::script_end);
            match end {
                Some(ScriptEnd::Victory(_)) if !matches!(self.screen, Screen::Victory) => next = Some(Screen::Victory),
                Some(ScriptEnd::Defeat(_)) if !matches!(self.screen, Screen::GameOver) => next = Some(Screen::GameOver),
                _ => {}
            }
        }
        if let Some(next) = next {
            if matches!(next, Screen::ScenarioSelect) {
                self.dialogs.clear();
            }
            if matches!(next, Screen::WorldMap) && !matches!(self.screen, Screen::WorldMap) {
                self.map_view.reset();
            }
            if matches!(self.screen, Screen::ClassSelect { .. }) {
                self.message = None;
            }
            self.screen = next;
        }
        // A new game may run on other content: draw its pictures.
        if let Some(g) = &self.game {
            if !Arc::ptr_eq(&g.content, self.assets.content()) {
                self.assets.set_content(g.content.clone());
            }
        }
    }
}
