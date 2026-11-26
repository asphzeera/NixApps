use serde::{Deserialize, Serialize};

/// Target Nix configuration scope managed by NixApps.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ConfigTarget {
    /// Home Manager configuration (`home.packages`, `programs.*`).
    HomeManager,
    /// NixOS system-wide configuration (`environment.systemPackages`, `programs.*`).
    NixosSystem,
}

impl ConfigTarget {
    /// Returns the Nix package list attribute path for this target.
    pub fn packages_attr(self) -> &'static str {
        match self {
            ConfigTarget::HomeManager => "home.packages",
            ConfigTarget::NixosSystem => "environment.systemPackages",
        }
    }

    /// Human-readable label for UI display.
    pub fn display_name(self) -> &'static str {
        match self {
            ConfigTarget::HomeManager => "Home Manager",
            ConfigTarget::NixosSystem => "NixOS System",
        }
    }
}

/// High-level store categories inferred from Nixpkgs metadata and (in Phase 3) AppStream.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Category {
    AudioVideo,
    Development,
    Education,
    Games,
    Graphics,
    Network,
    Office,
    Science,
    System,
    Utilities,
    Other,
}

impl Category {
    pub const ALL: &'static [Category] = &[
        Category::AudioVideo,
        Category::Development,
        Category::Education,
        Category::Games,
        Category::Graphics,
        Category::Network,
        Category::Office,
        Category::Science,
        Category::System,
        Category::Utilities,
        Category::Other,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Category::AudioVideo => "audio-video",
            Category::Development => "development",
            Category::Education => "education",
            Category::Games => "games",
            Category::Graphics => "graphics",
            Category::Network => "network",
            Category::Office => "office",
            Category::Science => "science",
            Category::System => "system",
            Category::Utilities => "utilities",
            Category::Other => "other",
        }
    }

    pub fn parse(value: &str) -> Self {
        match value {
            "audio-video" => Category::AudioVideo,
            "development" => Category::Development,
            "education" => Category::Education,
            "games" => Category::Games,
            "graphics" => Category::Graphics,
            "network" => Category::Network,
            "office" => Category::Office,
            "science" => Category::Science,
            "system" => Category::System,
            "utilities" => Category::Utilities,
            _ => Category::Other,
        }
    }

    pub fn display_name(self) -> &'static str {
        match self {
            Category::AudioVideo => "Audio & Video",
            Category::Development => "Development",
            Category::Education => "Education",
            Category::Games => "Games",
            Category::Graphics => "Graphics",
            Category::Network => "Internet & Network",
            Category::Office => "Office & Productivity",
            Category::Science => "Science",
            Category::System => "System",
            Category::Utilities => "Utilities & CLI",
            Category::Other => "Other",
        }
    }
}

/// A single package record stored in the local SQLite catalog.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackageRecord {
    /// Nixpkgs attribute path (e.g., `"obsidian"`, `"kdePackages.kdenlive"`).
    pub attr_path: String,
    /// Package base name (`pname`).
    pub pname: String,
    /// Package version string.
    pub version: String,
    /// Short one-line description (`meta.description`).
    pub description: Option<String>,
    /// Extended description (`meta.longDescription`).
    pub long_description: Option<String>,
    /// Primary homepage URL (`meta.homepage`).
    pub homepage: Option<String>,
    /// Human-readable license summary (`meta.license`).
    pub license: Option<String>,
    /// Main executable binary name (`meta.mainProgram`).
    pub main_program: Option<String>,
    /// Source definition position inside nixpkgs (e.g., `pkgs/by-name/...`).
    pub position: Option<String>,
    /// Store category.
    pub category: Category,
    /// Whether the package is marked unfree.
    pub unfree: bool,
    /// Whether the package is marked broken.
    pub broken: bool,
    /// Whether the package is marked insecure.
    pub insecure: bool,
}

/// Query parameters for searching the local package index.
#[derive(Debug, Clone, Default)]
pub struct SearchQuery {
    /// Free-text search term (empty returns top/browse results).
    pub text: String,
    /// Optional category filter.
    pub category: Option<Category>,
    /// Whether to include packages marked `unfree`. Defaults to `true`.
    pub include_unfree: bool,
    /// Whether to include packages marked `broken`. Defaults to `false`.
    pub include_broken: bool,
    /// Maximum number of results to return.
    pub limit: usize,
    /// Pagination offset.
    pub offset: usize,
}

impl SearchQuery {
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            category: None,
            include_unfree: true,
            include_broken: false,
            limit: 50,
            offset: 0,
        }
    }
}

/// Metadata about the last channel synchronization.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncInfo {
    pub channel: String,
    pub package_count: usize,
    pub synced_at_epoch: i64,
}

/// How a managed application is declared inside the generated `.nix` module.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum InstallMode {
    /// Standard entry inside `home.packages` or `environment.systemPackages`.
    Package,
    /// Standard package PLUS an additional custom Nix snippet (e.g., `programs.<app>` options).
    PackageWithConfig,
    /// Declared purely via custom Nix configuration (e.g., `programs.steam.enable = true;` or a custom override).
    CustomOnly,
}

/// An application managed by NixApps in the user's declarative state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManagedApp {
    /// Nixpkgs attribute path (e.g., `"obsidian"`, `"wineWow64Packages.staging"`).
    pub attr_path: String,
    /// How this app is emitted in the generated `.nix` module.
    pub mode: InstallMode,
    /// Optional user-defined Nix attribute block placed at module top level
    /// (e.g., `programs.helix = { enable = true; };`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub custom_nix: Option<String>,
    /// Optional user notes / reference URL (e.g., GitHub README link).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
}

/// Persistent declarative state stored in `nixapps-state.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManagedState {
    pub schema_version: u32,
    pub target: ConfigTarget,
    pub channel: String,
    #[serde(default)]
    pub apps: Vec<ManagedApp>,
}

impl ManagedState {
    pub const CURRENT_SCHEMA_VERSION: u32 = 1;

    pub fn new(target: ConfigTarget, channel: impl Into<String>) -> Self {
        Self {
            schema_version: Self::CURRENT_SCHEMA_VERSION,
            target,
            channel: channel.into(),
            apps: Vec::new(),
        }
    }
}

/// Cached external metadata (AppStream/Flathub/GitHub) stored in SQLite (used in Phase 3).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CachedMetadata {
    pub attr_path: String,
    pub source: String,
    pub icon_url: Option<String>,
    pub briefing: Option<String>,
    pub stars: Option<i64>,
    pub fetched_at_epoch: i64,
}
