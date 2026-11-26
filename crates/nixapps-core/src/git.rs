use std::path::{Path, PathBuf};
use std::process::Command;

use crate::error::{NixAppsError, Result};

/// Status of a managed Nix file inside a Git repository (important for Nix Flakes).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GitTrackingStatus {
    /// Directory is not inside a Git repository.
    NotAGitRepo,
    /// File is already tracked by Git (Flakes can evaluate it).
    Tracked { repo_root: PathBuf },
    /// File is untracked in a Git repo (Flakes will ignore it until staged).
    Untracked { repo_root: PathBuf },
}

/// Finds the Git repository root containing `path`, if any.
pub fn find_repo_root(path: &Path) -> Result<Option<PathBuf>> {
    let dir = if path.is_dir() {
        path
    } else {
        path.parent().unwrap_or(path)
    };

    if !dir.exists() {
        return Ok(None);
    }

    let output = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .map_err(|e| NixAppsError::Git {
            message: format!("failed to execute git: {e}"),
        })?;

    if !output.status.success() {
        return Ok(None);
    }

    let root = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if root.is_empty() {
        Ok(None)
    } else {
        Ok(Some(PathBuf::from(root)))
    }
}

/// Checks whether `file_path` is inside a Git repository and tracked by Git.
///
/// Nix Flakes ignore untracked files inside Git repositories, so detecting this
/// lets NixApps automatically run `git add -N` (intent-to-add) or notify the user.
pub fn check_tracking_status(file_path: &Path) -> Result<GitTrackingStatus> {
    let Some(repo_root) = find_repo_root(file_path)? else {
        return Ok(GitTrackingStatus::NotAGitRepo);
    };

    let output = Command::new("git")
        .arg("-C")
        .arg(&repo_root)
        .args(["ls-files", "--error-unmatch", "--"])
        .arg(file_path)
        .output()
        .map_err(|e| NixAppsError::Git {
            message: format!("failed to execute git ls-files: {e}"),
        })?;

    if output.status.success() {
        Ok(GitTrackingStatus::Tracked { repo_root })
    } else {
        Ok(GitTrackingStatus::Untracked { repo_root })
    }
}

/// Marks an untracked file as intent-to-add (`git add -N`) if it lives inside a Git repo.
///
/// Returns `Ok(true)` if `git add -N` was executed, or `Ok(false)` if not in a Git repo
/// or already tracked.
pub fn ensure_flake_visible(file_path: &Path) -> Result<bool> {
    match check_tracking_status(file_path)? {
        GitTrackingStatus::NotAGitRepo | GitTrackingStatus::Tracked { .. } => Ok(false),
        GitTrackingStatus::Untracked { repo_root } => {
            let output = Command::new("git")
                .arg("-C")
                .arg(&repo_root)
                .args(["add", "-N", "--"])
                .arg(file_path)
                .output()
                .map_err(|e| NixAppsError::Git {
                    message: format!("failed to execute git add -N: {e}"),
                })?;

            if !output.status.success() {
                return Err(NixAppsError::Git {
                    message: String::from_utf8_lossy(&output.stderr).trim().to_string(),
                });
            }

            Ok(true)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn non_git_directory_returns_not_a_git_repo() {
        let dir = tempdir().unwrap();
        let file = dir.path().join("nixapps-managed.nix");
        fs::write(&file, "{ ... }: {}\n").unwrap();

        assert_eq!(
            check_tracking_status(&file).unwrap(),
            GitTrackingStatus::NotAGitRepo
        );
        assert!(!ensure_flake_visible(&file).unwrap());
    }

    #[test]
    fn untracked_file_in_git_repo_becomes_visible_via_intent_to_add() {
        let dir = tempdir().unwrap();
        let status = Command::new("git")
            .arg("-C")
            .arg(dir.path())
            .args(["init", "-b", "main"])
            .status()
            .unwrap();
        assert!(status.success());

        let file = dir.path().join("nixapps-managed.nix");
        fs::write(&file, "{ ... }: {}\n").unwrap();

        assert!(matches!(
            check_tracking_status(&file).unwrap(),
            GitTrackingStatus::Untracked { .. }
        ));

        assert!(ensure_flake_visible(&file).unwrap());

        assert!(matches!(
            check_tracking_status(&file).unwrap(),
            GitTrackingStatus::Tracked { .. }
        ));
        // Second call is a no-op.
        assert!(!ensure_flake_visible(&file).unwrap());
    }
}
