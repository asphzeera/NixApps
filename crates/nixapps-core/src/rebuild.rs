use std::fs;
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;

use crate::error::{NixAppsError, Result};
use crate::models::ConfigTarget;

/// Configuration for running a NixOS / Home Manager system rebuild from NixApps.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RebuildCommand {
    /// Shell command executed to activate changes (e.g. `nixos-rebuild switch --flake /home/user/dotfiles#host`).
    pub command: String,
    /// Run via `pkexec sh -c <command>` for graphical Polkit privilege escalation.
    pub use_pkexec: bool,
}

/// Outcome of a completed rebuild run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RebuildOutcome {
    pub success: bool,
    pub exit_code: Option<i32>,
}

/// Reads the local Linux hostname (from `/etc/hostname` or `hostname` env) if available.
pub fn detect_hostname() -> Option<String> {
    if let Ok(raw) = fs::read_to_string("/etc/hostname") {
        let host = raw.trim().to_string();
        if !host.is_empty() {
            return Some(host);
        }
    }
    std::env::var("HOSTNAME")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Suggests a sensible default rebuild command based on the user's `config_dir` and `target`.
pub fn suggest_rebuild_command(config_dir: &Path, target: ConfigTarget) -> RebuildCommand {
    let has_flake = config_dir.join("flake.nix").is_file();
    let dir_str = config_dir.display();

    if has_flake {
        let host = detect_hostname().unwrap_or_else(|| "default".to_string());
        // Most NixOS + Flakes setups (including integrated home-manager NixOS modules)
        // activate via `nixos-rebuild switch --flake <dir>#<host>`.
        let command = format!("nixos-rebuild switch --flake '{dir_str}#{host}'");
        RebuildCommand {
            command,
            use_pkexec: true,
        }
    } else {
        match target {
            ConfigTarget::HomeManager => RebuildCommand {
                command: "home-manager switch".to_string(),
                use_pkexec: false,
            },
            ConfigTarget::NixosSystem => RebuildCommand {
                command: "nixos-rebuild switch".to_string(),
                use_pkexec: true,
            },
        }
    }
}

/// Picks a shell path listed in `/etc/shells` so `pkexec` never rejects transient
/// Nix devShell `$SHELL` values (e.g. `/nix/store/...-bash-interactive/bin/bash`).
pub fn resolve_valid_pkexec_shell() -> String {
    if let Ok(raw) = fs::read_to_string("/etc/shells") {
        let valid: Vec<&str> = raw
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
            .collect();
        if valid.contains(&"/bin/sh") {
            return "/bin/sh".to_string();
        }
        if let Some(first) = valid.first() {
            return (*first).to_string();
        }
    }
    "/bin/sh".to_string()
}

/// Executes `rebuild` and streams every combined stdout/stderr line to `on_line` in real time.
pub fn run_rebuild_streaming<F>(rebuild: &RebuildCommand, mut on_line: F) -> Result<RebuildOutcome>
where
    F: FnMut(String),
{
    let trimmed = rebuild.command.trim();
    if trimmed.is_empty() {
        return Err(NixAppsError::Sync(
            "rebuild command is empty — configure it in Setup first".to_string(),
        ));
    }

    let mut cmd = if rebuild.use_pkexec {
        // Ensure standard NixOS system binaries (`nixos-rebuild`, `nix`, `git`) are on PATH
        // even after `pkexec` sanitizes the environment, and force `$SHELL` to an entry in `/etc/shells`.
        let wrapped = format!(
            "export PATH=\"/run/wrappers/bin:/run/current-system/sw/bin:/nix/var/nix/profiles/default/bin:$PATH\"; {trimmed}"
        );
        let mut c = Command::new("pkexec");
        c.env("SHELL", resolve_valid_pkexec_shell());
        c.args(["/bin/sh", "-c", &wrapped]);
        c
    } else {
        let mut c = Command::new("sh");
        c.args(["-c", trimmed]);
        c
    };

    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());

    let mut child = cmd.spawn().map_err(|source| NixAppsError::Io {
        path: if rebuild.use_pkexec { "pkexec" } else { "sh" }.into(),
        source,
    })?;

    let stdout = child.stdout.take();
    let stderr = child.stderr.take();

    let (tx, rx) = mpsc::channel::<String>();

    let t_out = stdout.map(|out| {
        let tx = tx.clone();
        thread::spawn(move || {
            let reader = BufReader::new(out);
            for line in reader.lines().map_while(std::result::Result::ok) {
                let _ = tx.send(line);
            }
        })
    });

    let t_err = stderr.map(|err| {
        let tx = tx.clone();
        thread::spawn(move || {
            let reader = BufReader::new(err);
            for line in reader.lines().map_while(std::result::Result::ok) {
                let _ = tx.send(line);
            }
        })
    });

    drop(tx);

    for line in rx {
        on_line(line);
    }

    if let Some(h) = t_out {
        let _ = h.join();
    }
    if let Some(h) = t_err {
        let _ = h.join();
    }

    let status = child.wait().map_err(|source| NixAppsError::Io {
        path: "rebuild".into(),
        source,
    })?;

    Ok(RebuildOutcome {
        success: status.success(),
        exit_code: status.code(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn suggests_flake_rebuild_when_flake_nix_exists() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("flake.nix"), "{}\n").unwrap();

        let cmd = suggest_rebuild_command(dir.path(), ConfigTarget::HomeManager);
        assert!(cmd.use_pkexec);
        assert!(cmd.command.starts_with("nixos-rebuild switch --flake "));
    }

    #[test]
    fn suggests_non_flake_commands_when_no_flake_present() {
        let dir = tempdir().unwrap();
        let hm = suggest_rebuild_command(dir.path(), ConfigTarget::HomeManager);
        assert!(!hm.use_pkexec);
        assert_eq!(hm.command, "home-manager switch");

        let sys = suggest_rebuild_command(dir.path(), ConfigTarget::NixosSystem);
        assert!(sys.use_pkexec);
        assert_eq!(sys.command, "nixos-rebuild switch");
    }

    #[test]
    fn streaming_runner_captures_stdout_and_stderr() {
        let cmd = RebuildCommand {
            command: "echo hello-out && echo hello-err >&2".to_string(),
            use_pkexec: false,
        };
        let mut lines = Vec::new();
        let outcome = run_rebuild_streaming(&cmd, |line| lines.push(line)).unwrap();
        assert!(outcome.success);
        assert!(lines.iter().any(|l| l == "hello-out"));
        assert!(lines.iter().any(|l| l == "hello-err"));
    }

    #[test]
    fn pkexec_shell_is_listed_in_etc_shells_when_present() {
        let chosen = resolve_valid_pkexec_shell();
        assert!(!chosen.is_empty());
        if let Ok(raw) = fs::read_to_string("/etc/shells") {
            let listed = raw.lines().any(|line| line.trim() == chosen);
            assert!(listed, "expected `{chosen}` to be listed in /etc/shells");
        }
    }
}
