use std::process::Command;

use serde::Deserialize;

use crate::error::{NixAppsError, Result};
use crate::state::validate_attr_path;

/// Package dependency groups inspected on demand via local `nix eval`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PackageDependencies {
    #[serde(default)]
    pub build_inputs: Vec<String>,
    #[serde(default)]
    pub native_build_inputs: Vec<String>,
    #[serde(default)]
    pub propagated_build_inputs: Vec<String>,
}

impl PackageDependencies {
    /// Returns true if all dependency lists are empty.
    pub fn is_empty(&self) -> bool {
        self.build_inputs.is_empty()
            && self.native_build_inputs.is_empty()
            && self.propagated_build_inputs.is_empty()
    }

    /// Total deduplicated dependency count across all groups.
    pub fn total_count(&self) -> usize {
        self.build_inputs.len()
            + self.native_build_inputs.len()
            + self.propagated_build_inputs.len()
    }
}

const NIX_DEPS_APPLY_EXPR: &str = r#"p: let names = xs: if builtins.isList xs then builtins.filter (x: x != null) (map (x: if builtins.isAttrs x then (x.pname or x.name or null) else null) xs) else []; in { buildInputs = names (p.buildInputs or []); nativeBuildInputs = names (p.nativeBuildInputs or []); propagatedBuildInputs = names (p.propagatedBuildInputs or []); }"#;

/// Lazily inspects a package's real Nix dependencies using `nix eval --json`.
///
/// Uses `NIXPKGS_ALLOW_UNFREE=1` and `--impure` so unfree packages like `obsidian` or
/// `spotify` can also be inspected cleanly.
pub fn inspect_dependencies(attr_path: &str) -> Result<PackageDependencies> {
    validate_attr_path(attr_path)?;

    let installable = format!("nixpkgs#{attr_path}");
    let output = Command::new("nix")
        .env("NIXPKGS_ALLOW_UNFREE", "1")
        .args([
            "eval",
            "--impure",
            "--json",
            &installable,
            "--apply",
            NIX_DEPS_APPLY_EXPR,
        ])
        .output()
        .map_err(|source| NixAppsError::Io {
            path: "nix".into(),
            source,
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(NixAppsError::Sync(format!(
            "nix eval failed for `{attr_path}`: {stderr}"
        )));
    }

    let mut deps: PackageDependencies = serde_json::from_slice(&output.stdout)?;
    deps.build_inputs.sort();
    deps.build_inputs.dedup();
    deps.native_build_inputs.sort();
    deps.native_build_inputs.dedup();
    deps.propagated_build_inputs.sort();
    deps.propagated_build_inputs.dedup();

    Ok(deps)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_invalid_attr_paths_before_calling_nix() {
        assert!(inspect_dependencies("foo; rm -rf /").is_err());
        assert!(inspect_dependencies("").is_err());
    }
}
