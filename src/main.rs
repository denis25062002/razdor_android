mod ui;

use std::sync::Arc;

use macroquad::prelude::*;
use razdor::rules::content::Content;

use ui::assets::Assets;
use ui::App;

fn conf() -> Conf {
    Conf {
        window_title: "Razdor".to_owned(),
        window_width: 1280,
        window_height: 800,
        high_dpi: true,
        ..Default::default()
    }
}

#[macroquad::main(conf)]
async fn main() {
    ui::widgets::load_font().await;
    let content = Arc::new(Content::builtin());
    let mut app = App::new(Assets::load(content.clone()).await, content);
    loop {
        app.frame();
        next_frame().await;
    }
}
