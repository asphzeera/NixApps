use std::fs;
use std::io::Write;
use std::path::Path;

use tempfile::NamedTempFile;

use crate::error::{NixAppsError, Result};
use crate::models::{ConfigTarget, InstallMode, ManagedApp, ManagedState};

/// Validates a Nixpkgs attribute path (e.g. `obsidian`, `kdePackages.kdenlive`, `nerd-fonts.jetbrains-mono`).
pub fn validate_attr_path(attr_path: &str) -> Result<()> {
    if attr_path.is_empty() || attr_path.starts_with('.') || attr_path.ends_with('.') {
        return Err(NixAppsError::InvalidAttrPath(attr_path.to_string()));
    }

    for segment in attr_path.split('.') {
        if segment.is_empty() {
            return Err(NixAppsError::InvalidAttrPath(attr_path.to_string()));
        }
        // Standard Nix identifier characters (conservative for unquoted attr paths).
        let mut chars = segment.chars();
        let Some(first) = chars.next() else {
            return Err(NixAppsError::InvalidAttrPath(attr_path.to_string()));
        };
        if !(first.is_ascii_alphanumeric() || first == '_') {
            return Err(NixAppsError::InvalidAttrPath(attr_path.to_string()));
        }
        if !chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '\'' | '+')) {
            return Err(NixAppsError::InvalidAttrPath(attr_path.to_string()));
        }
    }

    Ok(())
}

/// Loads `ManagedState` from a JSON file.
pub fn load_state(path: &Path) -> Result<ManagedState> {
    let raw = fs::read_to_string(path).map_err(|source| NixAppsError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let mut state: ManagedState = serde_json::from_str(&raw)?;
    state.apps.sort_by(|a, b| a.attr_path.cmp(&b.attr_path));
    Ok(state)
}

/// Loads `ManagedState` if `path` exists, otherwise returns a fresh empty state.
pub fn load_or_init_state(
    path: &Path,
    target: ConfigTarget,
    channel: impl Into<String>,
) -> Result<ManagedState> {
    if path.exists() {
        load_state(path)
    } else {
        Ok(ManagedState::new(target, channel))
    }
}

/// Atomically writes `ManagedState` as pretty-printed JSON.
pub fn save_state(path: &Path, state: &ManagedState) -> Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent).map_err(|source| NixAppsError::Io {
        path: parent.to_path_buf(),
        source,
    })?;

    let mut normalized = state.clone();
    normalized
        .apps
        .sort_by(|a, b| a.attr_path.cmp(&b.attr_path));

    let json = serde_json::to_string_pretty(&normalized)? + "\n";

    let mut tmp = NamedTempFile::new_in(parent).map_err(|source| NixAppsError::Io {
        path: parent.to_path_buf(),
        source,
    })?;
    tmp.write_all(json.as_bytes())
        .map_err(|source| NixAppsError::Io {
            path: tmp.path().to_path_buf(),
            source,
        })?;
    tmp.flush().map_err(|source| NixAppsError::Io {
        path: tmp.path().to_path_buf(),
        source,
    })?;

    tmp.persist(path).map_err(|err| NixAppsError::Io {
        path: path.to_path_buf(),
        source: err.error,
    })?;

    Ok(())
}

impl ManagedState {
    /// Installs or updates a managed application entry (sorted deterministically by `attr_path`).
    pub fn upsert_app(&mut self, app: ManagedApp) -> Result<()> {
        validate_attr_path(&app.attr_path)?;

        if let Some(existing) = self
            .apps
            .iter_mut()
            .find(|item| item.attr_path == app.attr_path)
        {
            *existing = app;
        } else {
            self.apps.push(app);
            self.apps.sort_by(|a, b| a.attr_path.cmp(&b.attr_path));
        }

        Ok(())
    }

    /// Installs a simple package using `InstallMode::Package`.
    pub fn install_package(&mut self, attr_path: impl Into<String>) -> Result<()> {
        self.upsert_app(ManagedApp {
            attr_path: attr_path.into(),
            mode: InstallMode::Package,
            custom_nix: None,
            notes: None,
        })
    }

    /// Removes a managed application by `attr_path`. Returns `true` if removed.
    pub fn remove_app(&mut self, attr_path: &str) -> bool {
        let before = self.apps.len();
        self.apps.retain(|item| item.attr_path != attr_path);
        self.apps.len() != before
    }

    /// Returns whether `attr_path` is currently managed.
    pub fn is_installed(&self, attr_path: &str) -> bool {
        self.apps.iter().any(|item| item.attr_path == attr_path)
    }

    /// Finds a managed application by `attr_path`.
    pub fn get_app(&self, attr_path: &str) -> Option<&ManagedApp> {
        self.apps.iter().find(|item| item.attr_path == attr_path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn attr_path_validation_accepts_valid_and_rejects_invalid() {
        assert!(validate_attr_path("obsidian").is_ok());
        assert!(validate_attr_path("kdePackages.kdenlive").is_ok());
        assert!(validate_attr_path("nerd-fonts.jetbrains-mono").is_ok());
        assert!(validate_attr_path("wineWow64Packages.staging").is_ok());
        assert!(validate_attr_path("gtk+3").is_ok());

        assert!(validate_attr_path("").is_err());
        assert!(validate_attr_path(".obsidian").is_err());
        assert!(validate_attr_path("obsidian.").is_err());
        assert!(validate_attr_path("a..b").is_err());
        assert!(validate_attr_path("foo; bar").is_err());
        assert!(validate_attr_path("foo bar").is_err());
    }

    #[test]
    fn state_roundtrip_and_deterministic_sorting() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("nixapps-state.json");

        let mut state = ManagedState::new(ConfigTarget::HomeManager, "nixos-26.05");
        state.install_package("spotify").unwrap();
        state.install_package("obsidian").unwrap();
        state.install_package("qbittorrent").unwrap();

        save_state(&path, &state).unwrap();
        let loaded = load_state(&path).unwrap();

        let names: Vec<&str> = loaded.apps.iter().map(|a| a.attr_path.as_str()).collect();
        assert_eq!(names, vec!["obsidian", "qbittorrent", "spotify"]);
        assert!(loaded.is_installed("obsidian"));
        assert!(!loaded.is_installed("firefox"));
    }
}
