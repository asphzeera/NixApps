use std::env;
use std::fs;
use std::path::PathBuf;

use nixapps_core::{ConfigTarget, ManagedConfig, RebuildCommand, suggest_rebuild_command};
use serde::{Deserialize, Serialize};

/// Persistent GUI user settings stored in `~/.config/nixapps/settings.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppSettings {
    /// Path to the user's NixOS / Home Manager configuration folder (e.g. `~/av-dotfiles`).
    pub config_dir: Option<PathBuf>,
    /// Target scope (`HomeManager` or `NixosSystem`).
    pub target: ConfigTarget,
    /// NixOS channel to sync from `channels.nixos.org` (e.g. `nixos-26.05`, `nixos-unstable`).
    pub channel: String,
    /// Whether to include `unfree` packages in search results.
    #[serde(default = "default_true")]
    pub include_unfree: bool,
    /// Optional GitHub Personal Access Token to raise API rate limit from 60 to 5,000 req/h.
    #[serde(default)]
    pub github_token: Option<String>,
    /// Optional custom rebuild command (e.g. `nixos-rebuild switch --flake ~/av-dotfiles#avmonster`).
    #[serde(default)]
    pub rebuild_command: Option<String>,
    /// Whether to invoke the rebuild command through `pkexec` (Polkit) for root elevation.
    #[serde(default = "default_true")]
    pub rebuild_use_pkexec: bool,
}

fn default_true() -> bool {
    true
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            config_dir: None,
            target: ConfigTarget::HomeManager,
            channel: "nixos-26.05".to_string(),
            include_unfree: true,
            github_token: None,
            rebuild_command: None,
            rebuild_use_pkexec: true,
        }
    }
}

impl AppSettings {
    /// Returns whether onboarding has been configured with a valid directory.
    pub fn is_configured(&self) -> bool {
        self.config_dir.as_ref().is_some_and(|p| p.is_dir())
    }

    /// Builds a [`ManagedConfig`] bound to `config_dir`, if configured.
    pub fn managed_config(&self) -> Option<ManagedConfig> {
        self.config_dir.as_ref().map(ManagedConfig::default_in)
    }

    /// Returns the effective [`RebuildCommand`] (either user-configured or auto-suggested).
    pub fn effective_rebuild_command(&self) -> RebuildCommand {
        if let Some(cmd) = self.rebuild_command.as_deref()
            && !cmd.trim().is_empty()
        {
            return RebuildCommand {
                command: cmd.trim().to_string(),
                use_pkexec: self.rebuild_use_pkexec,
            };
        }

        if let Some(dir) = &self.config_dir {
            suggest_rebuild_command(dir, self.target)
        } else {
            RebuildCommand {
                command: match self.target {
                    ConfigTarget::HomeManager => "home-manager switch".to_string(),
                    ConfigTarget::NixosSystem => "nixos-rebuild switch".to_string(),
                },
                use_pkexec: matches!(self.target, ConfigTarget::NixosSystem),
            }
        }
    }

    /// Loads settings from `~/.config/nixapps/settings.json`, or returns defaults if missing.
    pub fn load() -> Self {
        let path = settings_file_path();
        if let Ok(raw) = fs::read_to_string(&path)
            && let Ok(parsed) = serde_json::from_str::<Self>(&raw)
        {
            return parsed;
        }
        Self::default()
    }

    /// Persists settings to `~/.config/nixapps/settings.json`.
    pub fn save(&self) -> std::io::Result<()> {
        let path = settings_file_path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        fs::write(path, format!("{json}\n"))
    }
}

/// Returns `~/.config/nixapps/settings.json` respecting `XDG_CONFIG_HOME`.
pub fn settings_file_path() -> PathBuf {
    let base = env::var("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            let home = env::var("HOME").unwrap_or_else(|_| ".".into());
            PathBuf::from(home).join(".config")
        });
    base.join("nixapps").join("settings.json")
}

/// Returns `~/.cache/nixapps` respecting `XDG_CACHE_HOME`.
pub fn cache_root_dir() -> PathBuf {
    let base = env::var("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            let home = env::var("HOME").unwrap_or_else(|_| ".".into());
            PathBuf::from(home).join(".cache")
        });
    base.join("nixapps")
}

/// Returns `~/.cache/nixapps/catalog.db`.
pub fn catalog_db_path() -> PathBuf {
    cache_root_dir().join("catalog.db")
}

/// Returns `~/.cache/nixapps/icons/`.
pub fn icons_cache_dir() -> PathBuf {
    cache_root_dir().join("icons")
}
