use std::io::Cursor;
use std::process::Command;

use nixapps_core::{
    CatalogDb, ConfigTarget, GitTrackingStatus, InstallMode, ManagedApp, ManagedConfig,
    SearchQuery, WriteOptions, check_tracking_status, sync_from_json_reader,
};
use tempfile::tempdir;

const FIXTURE_PACKAGES: &str = r#"{
  "packages": {
    "obsidian": {
      "pname": "obsidian",
      "version": "1.7.7",
      "meta": {
        "description": "Powerful knowledge base on top of local Markdown files",
        "homepage": "https://obsidian.md",
        "license": { "shortName": "unfree" },
        "position": "pkgs/by-name/ob/obsidian/package.nix:1",
        "unfree": true
      }
    },
    "spotify": {
      "pname": "spotify",
      "version": "1.2.50",
      "meta": {
        "description": "Play music from the Spotify music service",
        "homepage": "https://www.spotify.com",
        "license": { "shortName": "unfree" },
        "position": "pkgs/applications/audio/spotify/default.nix:1",
        "unfree": true
      }
    },
    "helix": {
      "pname": "helix",
      "version": "25.01",
      "meta": {
        "description": "Post-modern modal text editor",
        "homepage": "https://helix-editor.com",
        "license": { "spdxId": "MPL-2.0" },
        "position": "pkgs/applications/editors/helix/default.nix:1",
        "unfree": false
      }
    }
  }
}"#;

#[test]
fn full_declarative_workflow_sync_search_install_custom_config_and_git_flake() {
    let dir = tempdir().unwrap();

    // Initialize a Git-backed flake repo like ~/av-dotfiles.
    let init = Command::new("git")
        .arg("-C")
        .arg(dir.path())
        .args(["init", "-b", "main"])
        .status()
        .unwrap();
    assert!(init.success());

    // 1. Open persistent SQLite DB and stream fixture catalog into it.
    let db_path = dir.path().join("cache/index.db");
    let mut db = CatalogDb::open(&db_path).unwrap();
    let sync = sync_from_json_reader(
        &mut db,
        "nixos-26.05",
        Cursor::new(FIXTURE_PACKAGES.as_bytes()),
        |_| {},
    )
    .unwrap();
    assert_eq!(sync.package_count, 3);
    assert_eq!(db.sync_info().unwrap().unwrap().channel, "nixos-26.05");

    // 2. Search for packages.
    let hits = db.search(&SearchQuery::new("music")).unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].attr_path, "spotify");

    // 3. Bind ManagedConfig and install packages + custom Nix config.
    let cfg = ManagedConfig::default_in(dir.path());
    assert_eq!(cfg.relative_import_path(), "./modules/nixapps-managed.nix");

    let mut state = cfg
        .load_or_init(ConfigTarget::HomeManager, "nixos-26.05")
        .unwrap();
    state.install_package("obsidian").unwrap();
    state.install_package("spotify").unwrap();
    state
        .upsert_app(ManagedApp {
            attr_path: "helix".into(),
            mode: InstallMode::CustomOnly,
            custom_nix: Some("programs.helix.enable = true;".into()),
            notes: Some("Enabled via Home Manager module".into()),
        })
        .unwrap();

    // 4. Apply atomically with nix-instantiate --parse validation + Git flake staging.
    let report = cfg.apply(&state, &WriteOptions::default()).unwrap();
    assert!(report.git_staged_new_files);
    assert!(report.diff.contains("+    obsidian"));
    assert!(report.diff.contains("+    spotify"));
    assert!(report.diff.contains("+  programs.helix.enable = true;"));

    // Both generated files must be visible to Nix Flakes in Git.
    assert!(matches!(
        check_tracking_status(&report.module_path).unwrap(),
        GitTrackingStatus::Tracked { .. }
    ));
    assert!(matches!(
        check_tracking_status(&report.state_path).unwrap(),
        GitTrackingStatus::Tracked { .. }
    ));

    // 5. Reload state and uninstall one package; verify diff shows removal.
    let mut reloaded = cfg
        .load_or_init(ConfigTarget::HomeManager, "nixos-26.05")
        .unwrap();
    assert!(reloaded.remove_app("spotify"));
    let report2 = cfg.apply(&reloaded, &WriteOptions::default()).unwrap();
    assert!(report2.diff.contains("-    spotify"));
    assert!(!report2.git_staged_new_files);
}
