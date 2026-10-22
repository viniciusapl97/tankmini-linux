//! Colour tokens for dark and light, swapped at runtime.
//!
//! `style.css` only references `@tm_*` names; this module defines them for the
//! current scheme and keeps libadwaita's own colours in step, so popovers and
//! dialogs match the app surface.

use std::cell::{Cell, RefCell};

use gtk::gdk;
use tankmini_core::catalog::Role;

const DARK: &str = r#"
@define-color tm_bg #0b0b0d;
@define-color tm_surface #141417;
@define-color tm_surface_hi #1d1d21;
@define-color tm_hair alpha(#ffffff, 0.075);
@define-color tm_hair_hi alpha(#ffffff, 0.16);
@define-color tm_text #ececef;
@define-color tm_dim alpha(#ececef, 0.62);
@define-color tm_faint alpha(#ececef, 0.38);
@define-color tm_cta_bg #ececef;
@define-color tm_cta_fg #0b0b0d;
@define-color tm_ok #4ade80;
@define-color tm_warn #f5b14a;
@define-color tm_warn_soft alpha(#f5b14a, 0.16);
@define-color tm_live #f2555a;
@define-color tm_fam_fx #6fcf97;
@define-color tm_fam_amp #e8a95a;
@define-color tm_fam_mod #a89be8;
@define-color tm_fam_dly #6fa8e8;
@define-color tm_fam_rvb #5cc8bb;
@define-color tm_fam_cab #e8848f;
@define-color window_bg_color #0b0b0d;
@define-color view_bg_color #0b0b0d;
@define-color headerbar_bg_color #0b0b0d;
@define-color sidebar_bg_color #0b0b0d;
@define-color card_bg_color #141417;
@define-color popover_bg_color #17171a;
@define-color dialog_bg_color #111114;
@define-color accent_bg_color #ececef;
@define-color accent_fg_color #0b0b0d;
@define-color accent_color #ececef;
:root {
  --window-bg-color: #0b0b0d; --view-bg-color: #0b0b0d; --headerbar-bg-color: #0b0b0d;
  --sidebar-bg-color: #0b0b0d; --card-bg-color: #141417; --popover-bg-color: #17171a;
  --dialog-bg-color: #111114; --accent-bg-color: #ececef; --accent-fg-color: #0b0b0d;
  --accent-color: #ececef;
}
"#;

const LIGHT: &str = r#"
@define-color tm_bg #f5f5f6;
@define-color tm_surface #ffffff;
@define-color tm_surface_hi #ececee;
@define-color tm_hair alpha(#000000, 0.08);
@define-color tm_hair_hi alpha(#000000, 0.18);
@define-color tm_text #111114;
@define-color tm_dim alpha(#111114, 0.62);
@define-color tm_faint alpha(#111114, 0.40);
@define-color tm_cta_bg #111114;
@define-color tm_cta_fg #ffffff;
@define-color tm_ok #16a34a;
@define-color tm_warn #b7791f;
@define-color tm_warn_soft alpha(#d69e2e, 0.16);
@define-color tm_live #dc2f35;
@define-color tm_fam_fx #2f9e64;
@define-color tm_fam_amp #c27a1e;
@define-color tm_fam_mod #6f5fd0;
@define-color tm_fam_dly #3d7cc9;
@define-color tm_fam_rvb #1f9a8c;
@define-color tm_fam_cab #cc4f5d;
@define-color window_bg_color #f5f5f6;
@define-color view_bg_color #f5f5f6;
@define-color headerbar_bg_color #f5f5f6;
@define-color sidebar_bg_color #f5f5f6;
@define-color card_bg_color #ffffff;
@define-color popover_bg_color #ffffff;
@define-color dialog_bg_color #fafafa;
@define-color accent_bg_color #111114;
@define-color accent_fg_color #ffffff;
@define-color accent_color #111114;
:root {
  --window-bg-color: #f5f5f6; --view-bg-color: #f5f5f6; --headerbar-bg-color: #f5f5f6;
  --sidebar-bg-color: #f5f5f6; --card-bg-color: #ffffff; --popover-bg-color: #ffffff;
  --dialog-bg-color: #fafafa; --accent-bg-color: #111114; --accent-fg-color: #ffffff;
  --accent-color: #111114;
}
"#;

thread_local! {
    static TOKENS: RefCell<Option<gtk::CssProvider>> = const { RefCell::new(None) };
    static DARK_NOW: Cell<bool> = const { Cell::new(true) };
}

pub fn is_dark() -> bool {
    DARK_NOW.with(Cell::get)
}

/// Install the base stylesheet and the token provider, and follow the
/// libadwaita style manager from now on.
pub fn install(theme: &str) {
    let display = gdk::Display::default().expect("display");
    let base = gtk::CssProvider::new();
    base.load_from_string(include_str!("style.css"));
    gtk::style_context_add_provider_for_display(&display, &base, gtk::STYLE_PROVIDER_PRIORITY_APPLICATION);

    let tokens = gtk::CssProvider::new();
    gtk::style_context_add_provider_for_display(&display, &tokens, gtk::STYLE_PROVIDER_PRIORITY_APPLICATION + 1);
    TOKENS.with(|t| *t.borrow_mut() = Some(tokens));

    let manager = adw::StyleManager::default();
    manager.connect_dark_notify(|m| load_tokens(m.is_dark()));
    set_theme(theme);
}

/// `system`, `dark` or `light`.
pub fn set_theme(theme: &str) {
    let manager = adw::StyleManager::default();
    manager.set_color_scheme(match theme {
        "light" => adw::ColorScheme::ForceLight,
        "system" => adw::ColorScheme::Default,
        _ => adw::ColorScheme::ForceDark,
    });
    load_tokens(manager.is_dark());
}

fn load_tokens(dark: bool) {
    DARK_NOW.with(|d| d.set(dark));
    TOKENS.with(|t| {
        if let Some(p) = t.borrow().as_ref() {
            p.load_from_string(if dark { DARK } else { LIGHT });
        }
    });
}

/// Family accent as RGB, for Cairo drawing.
pub fn family_rgb(role: Role) -> (f64, f64, f64) {
    let hex = match (role, is_dark()) {
        (Role::Fx, true) => 0x6fcf97,
        (Role::Amp, true) => 0xe8a95a,
        (Role::Modulation, true) => 0xa89be8,
        (Role::Delay, true) => 0x6fa8e8,
        (Role::Reverb, true) => 0x5cc8bb,
        (Role::Cabinet, true) => 0xe8848f,
        (Role::Fx, false) => 0x2f9e64,
        (Role::Amp, false) => 0xc27a1e,
        (Role::Modulation, false) => 0x6f5fd0,
        (Role::Delay, false) => 0x3d7cc9,
        (Role::Reverb, false) => 0x1f9a8c,
        (Role::Cabinet, false) => 0xcc4f5d,
    };
    rgb(hex)
}

/// Neutral accent (macro knobs): the text colour.
pub fn neutral_rgb() -> (f64, f64, f64) {
    if is_dark() { rgb(0xececef) } else { rgb(0x111114) }
}

pub fn text_rgb() -> (f64, f64, f64) {
    neutral_rgb()
}

/// Knob track (the unlit part of the ring).
pub fn track_rgba() -> (f64, f64, f64, f64) {
    if is_dark() { (1.0, 1.0, 1.0, 0.08) } else { (0.0, 0.0, 0.0, 0.08) }
}

/// Knob face.
pub fn face_rgb() -> (f64, f64, f64) {
    if is_dark() { rgb(0x1a1a1e) } else { rgb(0xffffff) }
}

pub fn family_class(role: Role) -> &'static str {
    match role {
        Role::Fx => "fam-fx",
        Role::Amp => "fam-amp",
        Role::Modulation => "fam-mod",
        Role::Delay => "fam-dly",
        Role::Reverb => "fam-rvb",
        Role::Cabinet => "fam-cab",
    }
}

fn rgb(hex: u32) -> (f64, f64, f64) {
    (
        ((hex >> 16) & 0xff) as f64 / 255.0,
        ((hex >> 8) & 0xff) as f64 / 255.0,
        (hex & 0xff) as f64 / 255.0,
    )
}
