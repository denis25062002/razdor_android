//! macroquad presentation layer. Reads rules state and calls rules methods.
pub mod assets;
pub mod battle_view;
pub mod screens;
pub mod widgets;

use razdor::rules::game::Game;

use assets::Assets;
use battle_view::BattleView;

pub enum Screen {
    ClassSelect,
    WorldMap,
    Town,
    Battle(Box<BattleView>),
    GameOver,
    Victory,
}

pub struct App {
    pub assets: Assets,
    pub game: Option<Game>,
    pub screen: Screen,
    /// One-line notice shown on the world map / town screens.
    pub message: Option<String>,
}

impl App {
    pub fn new(assets: Assets) -> Self {
        App { assets, game: None, screen: Screen::ClassSelect, message: None }
    }

    pub fn frame(&mut self) {
        let next = match (&mut self.screen, &mut self.game) {
            (Screen::ClassSelect, game) => screens::class_select(game, &self.assets),
            (Screen::WorldMap, Some(game)) => screens::world_map(game, &self.assets, &mut self.message),
            (Screen::Town, Some(game)) => screens::town(game, &self.assets, &mut self.message),
            (Screen::Battle(view), Some(game)) => view.frame(game, &self.assets, &mut self.message),
            (Screen::GameOver, game) => screens::game_over(game),
            (Screen::Victory, game) => screens::victory(game),
            (_, None) => Some(Screen::ClassSelect),
        };
        if let Some(next) = next {
            self.screen = next;
        }
    }
}
