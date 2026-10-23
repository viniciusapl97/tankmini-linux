//! TANK MINI — desktop control app for the M-VAVE guitar pedal.
//!
//! Two modes over one model: "Tocar" (four macro knobs, quick toggles and
//! ready-made sounds, for people who just want to play) and "Editar" (every
//! block, parameter, chain order and raw byte). USB runs on a worker thread
//! (`tankmini_core::worker`); see `app.rs` for the editing model.

mod app;
mod audio;
mod dialogs;
mod edit;
mod i18n;
mod knob;
mod pedal;
mod play;
mod rail;
mod theme;
mod toggle;
mod tuner;
mod widgets;
mod window;

use adw::prelude::*;
use gtk::glib;

const APP_ID: &str = "dev.v1ni.TankMini";

fn main() -> glib::ExitCode {
    // A second, independent instance (screenshots, testing next to a running
    // copy) gets its own id instead of activating the first one.
    let id = match std::env::var("TANKMINI_INSTANCE") {
        Ok(suffix) if !suffix.is_empty() => format!("{APP_ID}.{suffix}"),
        _ => APP_ID.to_string(),
    };
    let application = adw::Application::builder().application_id(id).build();
    application.connect_activate(|application| {
        if let Some(win) = application.active_window() {
            win.present();
            return;
        }
        let window = adw::ApplicationWindow::new(application);
        let app = app::App::new(window.clone());
        theme::install(&app.state.borrow().settings.theme.clone());
        window::build(&app);
        rail::wire(&app);
        app.refresh_all();
        app.start();
        // Demo/screenshot instances must not touch the real audio.
        if std::env::var_os("TANKMINI_DEMO").is_none() {
            audio::resume(&app);
        }
        {
            let app = app.clone();
            application.connect_shutdown(move |_| app.shutdown());
        }
        window.present();
        if !app.state.borrow().settings.onboarded {
            let app = app.clone();
            glib::timeout_add_local_once(std::time::Duration::from_millis(600), move || {
                i18n::init(i18n::Lang::detect());
                dialogs::maybe_onboard(&app)
            });
        }
    });
    application.run()
}
