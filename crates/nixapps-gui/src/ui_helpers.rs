use std::path::Path;

use nixapps_core::{Category, InstallMode};
use relm4::adw;
use relm4::gtk;
use relm4::gtk::prelude::ButtonExt;

const NIXAPPS_LOGO_SVG: &[u8] = include_bytes!("../../../assets/io.github.nixapps.NixApps.svg");
const NIXOS_SNOWFLAKE_SVG: &[u8] = include_bytes!("../../../assets/nixos-snowflake.svg");

/// Subtle Libadwaita-harmonious CSS for store cards, category tiles, pill badges, and Nix code views.
pub const APP_CSS: &str = r#"
.badge {
    border-radius: 999px;
    padding: 2px 10px;
    font-size: 11px;
    font-weight: 700;
}

.badge-installed {
    background-color: alpha(@success_color, 0.18);
    color: @success_color;
}

.badge-unfree {
    background-color: alpha(@warning_color, 0.18);
    color: @warning_color;
}

.badge-stars {
    background-color: alpha(@accent_bg_color, 0.16);
    color: @accent_color;
}

.badge-category {
    background-color: alpha(@window_fg_color, 0.08);
    color: alpha(@window_fg_color, 0.8);
}

.badge-broken {
    background-color: alpha(@error_color, 0.18);
    color: @error_color;
}

.dep-chip {
    background-color: alpha(@window_fg_color, 0.07);
    border-radius: 8px;
    padding: 4px 10px;
    font-family: monospace;
    font-size: 12px;
}

.nix-code-view {
    font-family: monospace;
    font-size: 12.5px;
}

.snippet-card {
    background-color: alpha(@window_fg_color, 0.05);
    border-radius: 10px;
    padding: 12px;
}

.store-card {
    border-radius: 14px;
    padding: 12px;
}

.category-tile {
    border-radius: 12px;
    padding: 10px 14px;
}

.app-hero-icon {
    background-color: alpha(@accent_bg_color, 0.10);
    border-radius: 18px;
    padding: 10px;
}

.pending-pill {
    background-color: mix(@window_bg_color, @window_fg_color, 0.09);
    color: @window_fg_color;
    border: 1px solid mix(@window_bg_color, @accent_color, 0.45);
    border-radius: 999px;
    padding: 6px 8px 6px 16px;
    box-shadow: 0 6px 20px alpha(#000000, 0.35);
}

.pending-pill-dot {
    color: @accent_color;
}

.top-toast-pill {
    background-color: mix(@window_bg_color, @window_fg_color, 0.10);
    color: @window_fg_color;
    border: 1px solid mix(@window_bg_color, @window_fg_color, 0.22);
    border-radius: 14px;
    padding: 8px 10px 8px 14px;
    box-shadow: 0 6px 20px alpha(#000000, 0.32);
}

.template-chip {
    background-color: alpha(@accent_bg_color, 0.14);
    color: @accent_color;
    border: 1px solid alpha(@accent_color, 0.30);
    border-radius: 999px;
    padding: 5px 12px;
    font-weight: 600;
    font-size: 12px;
}

.template-chip:hover {
    background-color: alpha(@accent_bg_color, 0.26);
    border-color: alpha(@accent_color, 0.55);
}
"#;

/// Builds a button with both an icon and a text label using `adw::ButtonContent`.
///
/// In GTK4, setting `.icon_name()` and `.label()` on `gtk::Button` directly makes them
/// overwrite each other; `adw::ButtonContent` renders both cleanly side by side.
pub fn icon_button(icon_name: &str, label: &str, css_classes: &[&str]) -> gtk::Button {
    let content = adw::ButtonContent::builder()
        .icon_name(icon_name)
        .label(label)
        .build();
    gtk::Button::builder()
        .child(&content)
        .css_classes(css_classes.to_vec())
        .build()
}

/// Updates an icon+label button created via [`icon_button`].
pub fn set_icon_button(button: &gtk::Button, icon_name: &str, label: &str) {
    let content = adw::ButtonContent::builder()
        .icon_name(icon_name)
        .label(label)
        .build();
    button.set_child(Some(&content));
}

/// Decodes the embedded NixApps SVG logo into a `gdk::Texture` for Setup & About dialogs.
pub fn nixapps_logo_texture() -> Option<gtk::gdk::Texture> {
    let bytes = gtk::glib::Bytes::from_static(NIXAPPS_LOGO_SVG);
    gtk::gdk::Texture::from_bytes(&bytes).ok()
}

/// Decodes the embedded NixOS snowflake SVG into a `gdk::Texture` for the Setup hero.
pub fn nixos_snowflake_texture() -> Option<gtk::gdk::Texture> {
    let bytes = gtk::glib::Bytes::from_static(NIXOS_SNOWFLAKE_SVG);
    gtk::gdk::Texture::from_bytes(&bytes).ok()
}

/// Maps a store [`Category`] to a standard freedesktop/GNOME symbolic icon name.
pub fn category_icon_name(category: Category) -> &'static str {
    match category {
        Category::AudioVideo => "applications-multimedia-symbolic",
        Category::Development => "applications-engineering-symbolic",
        Category::Education => "applications-science-symbolic",
        Category::Games => "applications-games-symbolic",
        Category::Graphics => "applications-graphics-symbolic",
        Category::Network => "network-workgroup-symbolic",
        Category::Office => "x-office-document-symbolic",
        Category::Science => "applications-science-symbolic",
        Category::System => "preferences-system-symbolic",
        Category::Utilities => "utilities-terminal-symbolic",
        Category::Other => "application-x-executable-symbolic",
    }
}

/// Applies a cached raster/SVG icon if available, otherwise falls back cleanly to a category icon.
pub fn apply_app_icon(image: &gtk::Image, icon_path: Option<&Path>, category: Category, size: i32) {
    image.set_pixel_size(size);
    if let Some(path) = icon_path
        && path.is_file()
    {
        image.set_from_file(Some(path));
    } else {
        image.set_icon_name(Some(category_icon_name(category)));
    }
}

/// Human-readable label for [`InstallMode`].
pub fn install_mode_label(mode: &InstallMode) -> &'static str {
    match mode {
        InstallMode::Package => "Package",
        InstallMode::PackageWithConfig => "Package + Custom Nix",
        InstallMode::CustomOnly => "Custom Nix Only",
    }
}

/// Formats an integer with thousands separators (e.g. `144349` -> `"144,349"`).
pub fn format_count(n: usize) -> String {
    let s = n.to_string();
    let mut out = String::with_capacity(s.len() + s.len() / 3);
    for (i, ch) in s.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    out.chars().rev().collect()
}

/// Formats GitHub star counts compactly (e.g. `14250` -> `"★ 14.3k"`).
pub fn format_stars(stars: i64) -> String {
    if stars >= 1_000 {
        format!("★ {:.1}k", stars as f64 / 1_000.0)
    } else {
        format!("★ {stars}")
    }
}
