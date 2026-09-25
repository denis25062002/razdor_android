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

/// Debug: `RAZDOR_QUIT_AFTER=<frames>` asks to quit after that many frames, to test the exit
/// path without a window manager.
fn quit_after() -> Option<u64> {
    std::env::var("RAZDOR_QUIT_AFTER").ok()?.trim().parse().ok()
}

/// Ends the process without running the C `atexit` handlers.
///
/// A normal return from `main` crashed with SIGSEGV every time: `exit` runs the exit handlers
/// of the native libraries (GL / X11 / ALSA under miniquad), and one of them calls into code
/// that is no longer mapped. Everything Razdor writes (saves, `audio.json`) is written and
/// closed synchronously before this, so skipping the handlers loses nothing.
fn exit_now(app: &mut App) -> ! {
    app.shutdown();
    use std::io::Write;
    let _ = std::io::stdout().flush();
    let _ = std::io::stderr().flush();
    // SAFETY: `_exit` only ends the process; no Rust state is used after it.
    unsafe { libc::_exit(0) }
}

#[macroquad::main(conf)]
async fn main() {
    // Closing the window sets a flag instead of leaving the loop, so the exit goes through
    // `exit_now`.
    prevent_quit();
    ui::widgets::load_font().await;
    let content = Arc::new(Content::builtin());
    let mut app = App::new(Assets::load(content.clone()).await, content);
    let quit_after = quit_after();
    let mut frames = 0u64;
    loop {
        app.frame();
        frames += 1;
        if is_quit_requested() || quit_after.is_some_and(|n| frames >= n) {
            exit_now(&mut app);
        }
        next_frame().await;
    }
}
