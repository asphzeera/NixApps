use std::path::Path;

use nixapps_core::{Category, InstallMode};
use relm4::gtk;

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
"#;

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
