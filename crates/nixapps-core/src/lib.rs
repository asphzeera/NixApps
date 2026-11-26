//! # nixapps-core
//!
//! UI-agnostic core engine powering **NixApps** — a declarative App Store for
//! NixOS and Home Manager.
//!
//! ## Modules
//! - [`db`]: Local SQLite + FTS5 package catalog and metadata cache.
//! - [`sync`]: Streaming Brotli + JSON downloader/indexer for official NixOS `packages.json.br`.
//! - [`state`]: Persistent declarative state (`nixapps-state.json`) with attribute validation.
//! - [`nix_module`]: Deterministic `.nix` module generator, diff previewer, and `nix-instantiate --parse` validator.
//! - [`git`]: Git Flake repository detection and `git add -N` helper.

pub mod db;
pub mod deps;
pub mod error;
pub mod git;
pub mod metadata;
pub mod models;
pub mod nix_module;
pub mod rebuild;
pub mod state;
pub mod sync;

use std::path::{Path, PathBuf};

pub use db::CatalogDb;
pub use deps::{PackageDependencies, inspect_dependencies};
pub use error::{NixAppsError, Result};
pub use git::{GitTrackingStatus, check_tracking_status, ensure_flake_visible};
pub use metadata::{
    EnrichedPackageInfo, FEATURED_ATTRS, cached_icon_path, enrich_package, fetch_icon_cached,
    parse_github_repo,
};
pub use models::{
    CachedMetadata, Category, ConfigTarget, InstallMode, ManagedApp, ManagedState, PackageRecord,
    SearchQuery, SyncInfo,
};
pub use nix_module::{
    MANAGED_HEADER, WriteOptions, diff_strings, onboarding_import_snippet, preview_diff,
    render_nix_module, validate_nix_syntax, write_nix_module,
};
pub use rebuild::{
    RebuildCommand, RebuildOutcome, detect_hostname, run_rebuild_streaming, suggest_rebuild_command,
};
pub use state::{load_or_init_state, load_state, save_state, validate_attr_path};
pub use sync::{channel_packages_url, sync_channel, sync_from_json_reader};

/// Returns the crate version string.
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// Summary returned after applying declarative changes to disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApplyReport {
    pub state_path: PathBuf,
    pub module_path: PathBuf,
    pub diff: String,
    pub git_staged_new_files: bool,
    /// Non-fatal Git warning (e.g. `.git/objects` permission mismatch) — the Nix module
    /// and state files were still written and validated successfully.
    pub git_warning: Option<String>,
}

/// High-level coordinator binding a user's Nix configuration directory to NixApps'
/// managed state file (`nixapps-state.json`) and generated `.nix` module.
#[derive(Debug, Clone)]
pub struct ManagedConfig {
    pub config_dir: PathBuf,
    pub state_path: PathBuf,
    pub module_path: PathBuf,
}

impl ManagedConfig {
    /// Creates a standard binding inside `config_dir`:
    /// - `<config_dir>/modules/nixapps-managed.nix`
    /// - `<config_dir>/modules/nixapps-state.json`
    pub fn default_in(config_dir: impl Into<PathBuf>) -> Self {
        let config_dir = config_dir.into();
        let modules_dir = config_dir.join("modules");
        Self {
            state_path: modules_dir.join("nixapps-state.json"),
            module_path: modules_dir.join("nixapps-managed.nix"),
            config_dir,
        }
    }

    /// Creates a binding with explicit paths for the state and module files.
    pub fn custom(
        config_dir: impl Into<PathBuf>,
        state_path: impl Into<PathBuf>,
        module_path: impl Into<PathBuf>,
    ) -> Self {
        Self {
            config_dir: config_dir.into(),
            state_path: state_path.into(),
            module_path: module_path.into(),
        }
    }

    /// Loads the current managed state, or initializes a new one if none exists yet.
    pub fn load_or_init(
        &self,
        target: ConfigTarget,
        channel: impl Into<String>,
    ) -> Result<ManagedState> {
        load_or_init_state(&self.state_path, target, channel)
    }

    /// Computes a unified diff of what `apply` would change in the managed `.nix` module.
    pub fn preview_module_diff(&self, state: &ManagedState) -> Result<String> {
        let next = render_nix_module(state)?;
        preview_diff(&self.module_path, &next)
    }

    /// Validates, atomically writes both `nixapps-managed.nix` and `nixapps-state.json`,
    /// and attempts best-effort `git add -N` so Nix Flakes see newly created files.
    ///
    /// If Git staging fails (for example due to `.git/objects` owned by root), the write
    /// still succeeds and a non-fatal message is returned in `ApplyReport::git_warning`.
    pub fn apply(&self, state: &ManagedState, options: &WriteOptions) -> Result<ApplyReport> {
        let rendered = render_nix_module(state)?;
        let diff = preview_diff(&self.module_path, &rendered)?;

        // Write & validate the `.nix` module first — if syntax validation fails,
        // `nixapps-state.json` is never mutated.
        write_nix_module(&self.module_path, state, options)?;
        save_state(&self.state_path, state)?;

        let mut git_staged_new_files = false;
        let mut git_warnings: Vec<String> = Vec::new();

        match ensure_flake_visible(&self.module_path) {
            Ok(staged) => git_staged_new_files |= staged,
            Err(err) => git_warnings.push(err.to_string()),
        }
        match ensure_flake_visible(&self.state_path) {
            Ok(staged) => git_staged_new_files |= staged,
            Err(err) => git_warnings.push(err.to_string()),
        }

        let git_warning = if git_warnings.is_empty() {
            None
        } else {
            git_warnings.dedup();
            Some(format!(
                "Files saved, but `git add -N` failed ({}). If using Flakes, fix `.git` permissions or run `git add -N {}`.",
                git_warnings[0],
                self.relative_import_path()
            ))
        };

        Ok(ApplyReport {
            state_path: self.state_path.clone(),
            module_path: self.module_path.clone(),
            diff,
            git_staged_new_files,
            git_warning,
        })
    }

    /// Returns the relative Nix import path (e.g. `./modules/nixapps-managed.nix`) from `config_dir`.
    pub fn relative_import_path(&self) -> String {
        let rel = self
            .module_path
            .strip_prefix(&self.config_dir)
            .unwrap_or(Path::new("modules/nixapps-managed.nix"));
        format!("./{}", rel.to_string_lossy().replace('\\', "/"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_is_non_empty() {
        assert!(!version().is_empty());
    }
}
