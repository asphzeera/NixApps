use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::Command;

use similar::TextDiff;
use tempfile::NamedTempFile;

use crate::error::{NixAppsError, Result};
use crate::models::{ConfigTarget, InstallMode, ManagedState};
use crate::state::validate_attr_path;

/// Marker header placed at line 1 of every Nix file managed by NixApps.
///
/// NixApps refuses to overwrite an existing file unless this header is present,
/// ensuring hand-written user dotfiles can never be accidentally clobbered.
pub const MANAGED_HEADER: &str =
    "# Managed by NixApps (https://github.com/asphzeera/NixApps) — DO NOT EDIT MANUALLY";
const MANAGED_HEADER_PREFIX: &str = "# Managed by NixApps";

/// Options controlling how a managed `.nix` module is written to disk.
#[derive(Debug, Clone)]
pub struct WriteOptions {
    /// Validate generated syntax with `nix-instantiate --parse` before persisting.
    pub validate_with_nix: bool,
    /// Allow overwriting an existing file even if it does not contain `MANAGED_HEADER`.
    /// Defaults to `false` for safety.
    pub force_overwrite_unmanaged: bool,
}

impl Default for WriteOptions {
    fn default() -> Self {
        Self {
            validate_with_nix: true,
            force_overwrite_unmanaged: false,
        }
    }
}

/// Formats a Nix attribute path safely for use inside `with pkgs; [ ... ];`.
///
/// Attribute paths whose segments contain special characters (like `+` in `gtk+3`)
/// are quoted on the affected segment so Nix parses them reliably.
fn format_nix_attr_expr(attr_path: &str) -> String {
    attr_path
        .split('.')
        .map(|seg| {
            let needs_quotes =
                seg.chars().next().is_some_and(|c| c.is_ascii_digit()) || seg.contains('+');
            if needs_quotes {
                format!("pkgs.\"{seg}\"")
            } else {
                seg.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join(".")
}

/// Indents every non-empty line in `block` by `spaces` spaces.
fn indent_block(block: &str, spaces: usize) -> String {
    let prefix = " ".repeat(spaces);
    block
        .lines()
        .map(|line| {
            if line.trim().is_empty() {
                String::new()
            } else {
                format!("{prefix}{line}")
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Deterministically renders a complete Nix module from `ManagedState`.
///
/// The generated module uses `config = lib.mkMerge` with `options ? home` and
/// `options ? environment` guards so it works safely whether imported in
/// **Home Manager** (`home.nix`) or **NixOS System** (`configuration.nix`),
/// preventing `option 'environment' does not exist` or `option 'home' does not exist` errors.
pub fn render_nix_module(state: &ManagedState) -> Result<String> {
    let mut standard_pkgs: Vec<&str> = Vec::new();
    let mut custom_blocks: Vec<(&str, &str)> = Vec::new();

    for app in &state.apps {
        validate_attr_path(&app.attr_path)?;

        match app.mode {
            InstallMode::Package => {
                standard_pkgs.push(app.attr_path.as_str());
            }
            InstallMode::PackageWithConfig => {
                standard_pkgs.push(app.attr_path.as_str());
                if let Some(snippet) = app.custom_nix.as_deref()
                    && !snippet.trim().is_empty()
                {
                    custom_blocks.push((app.attr_path.as_str(), snippet.trim()));
                }
            }
            InstallMode::CustomOnly => {
                if let Some(snippet) = app.custom_nix.as_deref()
                    && !snippet.trim().is_empty()
                {
                    custom_blocks.push((app.attr_path.as_str(), snippet.trim()));
                }
            }
        }
    }

    standard_pkgs.sort_unstable();
    standard_pkgs.dedup();
    custom_blocks.sort_by_key(|(attr, _)| *attr);

    let mut out = String::new();
    out.push_str(MANAGED_HEADER);
    out.push('\n');
    out.push_str(&format!(
        "# Target: {} | Channel: {}\n",
        state.target.display_name(),
        state.channel
    ));
    out.push_str(
        "# Auto-adapts to both Home Manager (`home.packages`) and NixOS (`environment.systemPackages`).\n",
    );
    out.push_str("{ pkgs, lib, options, ... }:\n");
    out.push_str("let\n");

    if standard_pkgs.is_empty() {
        out.push_str("  managedPackages = [ ];\n");
    } else {
        out.push_str("  managedPackages = with pkgs; [\n");
        for pkg in &standard_pkgs {
            let expr = format_nix_attr_expr(pkg);
            out.push_str(&format!("    {expr}\n"));
        }
        out.push_str("  ];\n");
    }

    out.push_str("in\n{\n");
    out.push_str("  config = lib.mkMerge [\n");
    out.push_str("    (lib.optionalAttrs (options ? home && options.home ? packages) {\n");
    out.push_str("      home.packages = managedPackages;\n");
    out.push_str("    })\n");
    out.push_str(
        "    (lib.optionalAttrs (options ? environment && options.environment ? systemPackages) {\n",
    );
    out.push_str("      environment.systemPackages = managedPackages;\n");
    out.push_str("    })\n");

    if !custom_blocks.is_empty() {
        out.push_str("    {\n");
        out.push_str("      # --- Custom per-application configurations ---\n");
        for (attr, snippet) in custom_blocks {
            out.push_str(&format!("      # [nixapps:{attr}]\n"));
            out.push_str(&indent_block(snippet, 6));
            out.push_str("\n\n");
        }
        if out.ends_with("\n\n") {
            out.pop();
        }
        out.push_str("    }\n");
    }

    out.push_str("  ];\n");
    out.push_str("}\n");
    Ok(out)
}

/// Validates a Nix file's syntax using `nix-instantiate --parse`.
pub fn validate_nix_syntax(file_path: &Path) -> Result<()> {
    let output = Command::new("nix-instantiate")
        .arg("--parse")
        .arg(file_path)
        .output()
        .map_err(|source| NixAppsError::Io {
            path: file_path.to_path_buf(),
            source,
        })?;

    if !output.status.success() {
        return Err(NixAppsError::NixValidationFailed {
            stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
        });
    }

    Ok(())
}

/// Returns `true` if `path` already exists and starts with the NixApps managed header.
pub fn is_managed_nix_file(path: &Path) -> Result<bool> {
    if !path.exists() {
        return Ok(false);
    }
    let content = fs::read_to_string(path).map_err(|source| NixAppsError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    Ok(content.starts_with(MANAGED_HEADER_PREFIX))
}

/// Produces a unified diff between two Nix content strings.
pub fn diff_strings(old_content: &str, new_content: &str, file_label: &str) -> String {
    let diff = TextDiff::from_lines(old_content, new_content);
    diff.unified_diff()
        .context_radius(3)
        .header(&format!("a/{file_label}"), &format!("b/{file_label}"))
        .to_string()
}

/// Produces a unified diff between the existing file on disk (or empty) and `new_content`.
pub fn preview_diff(path: &Path, new_content: &str) -> Result<String> {
    let old_content = if path.exists() {
        fs::read_to_string(path).map_err(|source| NixAppsError::Io {
            path: path.to_path_buf(),
            source,
        })?
    } else {
        String::new()
    };

    let file_label = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("nixapps-managed.nix");

    Ok(diff_strings(old_content.as_str(), new_content, file_label))
}

/// Renders, validates, and atomically writes the managed `.nix` module to `target_path`.
///
/// Guarantees:
/// 1. Refuses to overwrite an existing file without `MANAGED_HEADER` unless `force_overwrite_unmanaged` is true.
/// 2. Writes to a temporary file in the same directory first.
/// 3. Validates the temporary file via `nix-instantiate --parse` (when enabled).
/// 4. Atomically renames the temporary file over `target_path`.
pub fn write_nix_module(
    target_path: &Path,
    state: &ManagedState,
    options: &WriteOptions,
) -> Result<String> {
    if target_path.exists()
        && !options.force_overwrite_unmanaged
        && !is_managed_nix_file(target_path)?
    {
        return Err(NixAppsError::UnmanagedFileExists {
            path: target_path.to_path_buf(),
        });
    }

    let rendered = render_nix_module(state)?;

    let parent = target_path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent).map_err(|source| NixAppsError::Io {
        path: parent.to_path_buf(),
        source,
    })?;

    let mut tmp = NamedTempFile::new_in(parent).map_err(|source| NixAppsError::Io {
        path: parent.to_path_buf(),
        source,
    })?;
    tmp.write_all(rendered.as_bytes())
        .map_err(|source| NixAppsError::Io {
            path: tmp.path().to_path_buf(),
            source,
        })?;
    tmp.flush().map_err(|source| NixAppsError::Io {
        path: tmp.path().to_path_buf(),
        source,
    })?;

    if options.validate_with_nix {
        validate_nix_syntax(tmp.path())?;
    }

    tmp.persist(target_path).map_err(|err| NixAppsError::Io {
        path: target_path.to_path_buf(),
        source: err.error,
    })?;

    Ok(rendered)
}

/// Returns the copy-paste `imports` snippet users add to `home.nix` or `configuration.nix`.
pub fn onboarding_import_snippet(relative_module_path: &str, target: ConfigTarget) -> String {
    let target_file = match target {
        ConfigTarget::HomeManager => "home.nix",
        ConfigTarget::NixosSystem => "configuration.nix",
    };
    format!(
        "# Add this line inside `imports = [ ... ];` in your {target_file}:\n  {relative_module_path}"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::ManagedApp;
    use tempfile::tempdir;

    #[test]
    fn renders_valid_home_manager_and_nixos_modules() {
        let mut hm = ManagedState::new(ConfigTarget::HomeManager, "nixos-26.05");
        hm.install_package("spotify").unwrap();
        hm.install_package("obsidian").unwrap();
        hm.upsert_app(ManagedApp {
            attr_path: "helix".into(),
            mode: InstallMode::CustomOnly,
            custom_nix: Some("programs.helix = {\n  enable = true;\n};".into()),
            notes: None,
        })
        .unwrap();

        let dir = tempdir().unwrap();
        let hm_path = dir.path().join("nixapps-home.nix");
        let rendered = write_nix_module(&hm_path, &hm, &WriteOptions::default()).unwrap();

        assert!(rendered.contains("managedPackages = with pkgs; ["));
        assert!(rendered.contains("    obsidian\n    spotify"));
        assert!(rendered.contains("options ? home && options.home ? packages"));
        assert!(rendered.contains("options ? environment && options.environment ? systemPackages"));
        assert!(rendered.contains("      programs.helix = {"));
        assert!(is_managed_nix_file(&hm_path).unwrap());

        let mut sys = ManagedState::new(ConfigTarget::NixosSystem, "nixos-26.05");
        sys.install_package("btop").unwrap();
        let sys_path = dir.path().join("nixapps-system.nix");
        let sys_rendered = write_nix_module(&sys_path, &sys, &WriteOptions::default()).unwrap();
        assert!(sys_rendered.contains("environment.systemPackages = managedPackages;"));
        assert!(sys_rendered.contains("home.packages = managedPackages;"));
    }

    #[test]
    fn refuses_to_overwrite_unmanaged_file() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("custom.nix");
        fs::write(&path, "{ pkgs, ... }: { home.packages = []; }\n").unwrap();

        let state = ManagedState::new(ConfigTarget::HomeManager, "nixos-26.05");
        let err = write_nix_module(&path, &state, &WriteOptions::default()).unwrap_err();
        assert!(matches!(err, NixAppsError::UnmanagedFileExists { .. }));
    }

    #[test]
    fn rejects_invalid_custom_nix_syntax_without_touching_target() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("nixapps-managed.nix");

        // First write a valid module.
        let mut valid_state = ManagedState::new(ConfigTarget::HomeManager, "nixos-26.05");
        valid_state.install_package("obsidian").unwrap();
        let original = write_nix_module(&path, &valid_state, &WriteOptions::default()).unwrap();

        // Now try writing broken custom Nix syntax.
        let mut broken_state = valid_state.clone();
        broken_state
            .upsert_app(ManagedApp {
                attr_path: "broken-demo".into(),
                mode: InstallMode::CustomOnly,
                custom_nix: Some("programs.broken = { enable = ;".into()),
                notes: None,
            })
            .unwrap();

        let err = write_nix_module(&path, &broken_state, &WriteOptions::default()).unwrap_err();
        assert!(matches!(err, NixAppsError::NixValidationFailed { .. }));

        // Original file on disk must remain untouched!
        let on_disk = fs::read_to_string(&path).unwrap();
        assert_eq!(on_disk, original);
    }

    #[test]
    fn preview_diff_shows_added_packages() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("nixapps-managed.nix");

        let mut state = ManagedState::new(ConfigTarget::HomeManager, "nixos-26.05");
        state.install_package("obsidian").unwrap();
        write_nix_module(&path, &state, &WriteOptions::default()).unwrap();

        state.install_package("spotify").unwrap();
        let next = render_nix_module(&state).unwrap();
        let diff = preview_diff(&path, &next).unwrap();

        assert!(diff.contains("+    spotify"));
    }
}
