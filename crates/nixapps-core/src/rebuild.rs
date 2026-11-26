use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;

use serde::{Deserialize, Serialize};

use crate::error::{NixAppsError, Result};
use crate::models::ConfigTarget;

const NIXOS_SAFE_PATH: &str =
    "/run/wrappers/bin:/run/current-system/sw/bin:/nix/var/nix/profiles/default/bin";

/// How NixApps acquires elevated privileges when running system rebuild commands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ElevationMethod {
    /// Uses NixApps' built-in GTK4 password dialog and pipes password securely to `sudo -S`.
    /// Works reliably on every Wayland compositor (Niri, Hyprland, Sway, GNOME, KDE) even
    /// without a running Polkit authentication agent.
    BuiltInSudo,
    /// Uses desktop Polkit (`pkexec`) with a validated `/etc/shells` entry.
    Polkit,
    /// Executes directly as the current user (e.g. standalone `home-manager switch`).
    None,
}

/// Configuration for running a NixOS / Home Manager system rebuild from NixApps.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RebuildCommand {
    /// Shell command executed to activate changes (e.g. `nixos-rebuild switch --flake /home/user/dotfiles#host`).
    pub command: String,
    /// Privilege elevation strategy.
    pub elevation: ElevationMethod,
}

/// Outcome of a completed rebuild run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RebuildOutcome {
    pub success: bool,
    pub exit_code: Option<i32>,
}

/// Reads the local Linux hostname (from `/etc/hostname` or `HOSTNAME` env) if available.
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
///
/// Detection is strictly based on standard NixOS/Home Manager files (never shell aliases):
/// - If `flake.nix` exists and defines `homeConfigurations` without `nixosConfigurations`,
///   suggests `home-manager switch --flake '<config_dir>'` (no root elevation needed).
/// - If `flake.nix` exists (standard NixOS or NixOS + integrated Home Manager module),
///   suggests `nixos-rebuild switch --flake '<config_dir>#<hostname>'` with `BuiltInSudo`.
/// - Otherwise falls back to classic `home-manager switch` or `nixos-rebuild switch`.
pub fn suggest_rebuild_command(config_dir: &Path, target: ConfigTarget) -> RebuildCommand {
    let flake_path = config_dir.join("flake.nix");
    let dir_str = config_dir.display();

    if flake_path.is_file() {
        let flake_src = fs::read_to_string(&flake_path).unwrap_or_default();
        let has_nixos = flake_src.contains("nixosConfigurations");
        let has_hm = flake_src.contains("homeConfigurations");

        if has_hm && !has_nixos {
            return RebuildCommand {
                command: format!("home-manager switch --flake '{dir_str}'"),
                elevation: ElevationMethod::None,
            };
        }

        let host = detect_hostname().unwrap_or_else(|| "default".to_string());
        let command = format!("nixos-rebuild switch --flake '{dir_str}#{host}'");
        RebuildCommand {
            command,
            elevation: ElevationMethod::BuiltInSudo,
        }
    } else {
        match target {
            ConfigTarget::HomeManager => RebuildCommand {
                command: "home-manager switch".to_string(),
                elevation: ElevationMethod::None,
            },
            ConfigTarget::NixosSystem => RebuildCommand {
                command: "nixos-rebuild switch".to_string(),
                elevation: ElevationMethod::BuiltInSudo,
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
///
/// When `rebuild.elevation == ElevationMethod::BuiltInSudo`, `sudo_password` is written
/// directly to `sudo -S`'s private stdin pipe and never printed or persisted.
pub fn run_rebuild_streaming<F>(
    rebuild: &RebuildCommand,
    sudo_password: Option<&str>,
    mut on_line: F,
) -> Result<RebuildOutcome>
where
    F: FnMut(String),
{
    let trimmed = rebuild.command.trim();
    if trimmed.is_empty() {
        return Err(NixAppsError::Sync(
            "rebuild command is empty — configure it in Setup first".to_string(),
        ));
    }

    let wrapped = format!("export PATH=\"{NIXOS_SAFE_PATH}:$PATH\"; {trimmed}");

    let (mut cmd, bin_name, feed_password) = match rebuild.elevation {
        ElevationMethod::BuiltInSudo => {
            let mut c = Command::new("sudo");
            // `-S` reads password from stdin, `-k` ensures clean auth check, `-p ""` suppresses prompt noise.
            c.args(["-S", "-p", "", "/bin/sh", "-c", &wrapped]);
            c.stdin(Stdio::piped());
            (c, "sudo", true)
        }
        ElevationMethod::Polkit => {
            let mut c = Command::new("pkexec");
            c.env("SHELL", resolve_valid_pkexec_shell());
            // Detached stdin prevents pkexec from falling back to a terminal TTY prompt.
            c.stdin(Stdio::null());
            c.args(["/bin/sh", "-c", &wrapped]);
            (c, "pkexec", false)
        }
        ElevationMethod::None => {
            let mut c = Command::new("/bin/sh");
            c.stdin(Stdio::null());
            c.args(["-c", &wrapped]);
            (c, "/bin/sh", false)
        }
    };

    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());

    let mut child = cmd.spawn().map_err(|source| NixAppsError::Io {
        path: bin_name.into(),
        source,
    })?;

    if feed_password && let Some(mut stdin) = child.stdin.take() {
        let pw = sudo_password.unwrap_or_default();
        let _ = writeln!(stdin, "{pw}");
        let _ = stdin.flush();
        // Dropping `stdin` closes the pipe cleanly after delivering the password.
    }

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
        fs::write(
            dir.path().join("flake.nix"),
            "{ outputs = { ... }: { nixosConfigurations.host = {}; }; }\n",
        )
        .unwrap();

        let cmd = suggest_rebuild_command(dir.path(), ConfigTarget::HomeManager);
        assert_eq!(cmd.elevation, ElevationMethod::BuiltInSudo);
        assert!(cmd.command.starts_with("nixos-rebuild switch --flake "));
    }

    #[test]
    fn suggests_standalone_home_manager_flake_command() {
        let dir = tempdir().unwrap();
        fs::write(
            dir.path().join("flake.nix"),
            "{ outputs = { ... }: { homeConfigurations.user = {}; }; }\n",
        )
        .unwrap();

        let cmd = suggest_rebuild_command(dir.path(), ConfigTarget::HomeManager);
        assert_eq!(cmd.elevation, ElevationMethod::None);
        assert!(cmd.command.starts_with("home-manager switch --flake "));
    }

    #[test]
    fn suggests_non_flake_commands_when_no_flake_present() {
        let dir = tempdir().unwrap();
        let hm = suggest_rebuild_command(dir.path(), ConfigTarget::HomeManager);
        assert_eq!(hm.elevation, ElevationMethod::None);
        assert_eq!(hm.command, "home-manager switch");

        let sys = suggest_rebuild_command(dir.path(), ConfigTarget::NixosSystem);
        assert_eq!(sys.elevation, ElevationMethod::BuiltInSudo);
        assert_eq!(sys.command, "nixos-rebuild switch");
    }

    #[test]
    fn streaming_runner_captures_stdout_and_stderr() {
        let cmd = RebuildCommand {
            command: "echo hello-out && echo hello-err >&2".to_string(),
            elevation: ElevationMethod::None,
        };
        let mut lines = Vec::new();
        let outcome = run_rebuild_streaming(&cmd, None, |line| lines.push(line)).unwrap();
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
