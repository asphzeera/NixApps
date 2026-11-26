# Project Management Log — NixApps

This document serves as the living engineering and project management journal for **NixApps**. Every phase, major milestone, blocker, and key decision is logged chronologically so anyone following or contributing to the repository can trace how the project evolved.

---

## Entry #001 — 2026-11-25 — Phase 0: Project Kickoff, Feasibility & Repository Foundation

- **Phase:** Phase 0 (Foundation & Governance)
- **Participants:** Human Maintainer (Asaph) + AI Engineering Assistant (Google Antigravity)
- **Status:** Completed

### Objective
Validate technical feasibility of a declarative desktop App Store for NixOS/Home Manager, select the optimal stack, and establish a production-grade open-source repository structure with transparent AI-assisted governance.

### Key Decisions Made
1. **Project Name:** `NixApps` (located in `~/dev/NixApps`).
2. **Declarative Architecture ([ADR-0001](adr/0001-managed-nix-module.md)):** Adopted a **dedicated managed `.nix` module** imported by a single line in the user's config, rather than AST-editing arbitrary hand-written `.nix` files. Supports both **Home Manager** and **NixOS System** from MVP.
3. **Technology Stack ([ADR-0002](adr/0002-rust-gtk4-relm4-stack.md)):** Selected **Rust + GTK4 / Libadwaita (Relm4)** for native Wayland integration, zero WebView overhead, and minimal RAM footprint (tested against an 8 GB RAM NixOS + Niri workstation).
4. **Metadata Strategy ([ADR-0003](adr/0003-metadata-sources-without-scraping.md)):** Eliminated fragile HTML scraping in favor of official `packages.json.br` indexed in SQLite FTS5, AppStream/Flathub APIs for icons/categories, cached on-demand GitHub REST API for briefings, and local `nix` CLI for dependencies.
5. **License & Open AI Governance ([ADR-0004](adr/0004-ai-assisted-development-governance.md)):** Chose **MIT License** and published `AI_TRANSPARENCY.md`, `AGENTS.md`, PR disclosure templates, and this log.

### Deliverables Created
- Cargo workspace (`crates/nixapps-core`, `crates/nixapps-gui`) and `flake.nix` devShell.
- Complete documentation suite (`README.md`, `README.pt-BR.md`, `PRD.md`, `ARCHITECTURE.md`, `ROADMAP.md`, `CHANGELOG.md`, `AI_TRANSPARENCY.md`, `AGENTS.md`, `CONTRIBUTING.md`, `CODE_OF_CONDUCT.md`, ADRs `0001`–`0004`).
- GitHub `.github/` templates and CI workflow.

### Next Steps
- Begin **Phase 1 (`nixapps-core`)**: implement `packages.json.br` streaming sync into SQLite FTS5, search API, managed `.nix` generator, `nix-instantiate --parse` validator, and Git Flake helper.

---

## Entry #002 — 2026-11-25 — Phase 1: Core Engine (`nixapps-core`) Complete

- **Phase:** Phase 1 (Core Engine)
- **Participants:** Human Maintainer (Asaph) + AI Engineering Assistant (Google Antigravity)
- **Status:** Completed

### Objective
Build and verify the complete UI-agnostic core engine (`crates/nixapps-core`) covering streaming channel sync, local SQLite FTS5 search, deterministic `.nix` module generation, syntax validation, and Git Flake integration.

### Deliverables Implemented
- **`db.rs`:** SQLite WAL + FTS5 schema (`packages`, `packages_fts`, `sync_meta`, `metadata_cache`), transactional streaming writer, exact/prefix + BM25 ranked search, category counts, and safe FTS5 query sanitization.
- **`sync.rs`:** Streaming `reqwest` → `brotli::Decompressor` → `serde::de::DeserializeSeed` parser for official `channels.nixos.org/<channel>/packages.json.br` handling polymorphic Nixpkgs fields (`homepage` string/array, `license` string/object/array) with near-constant RAM usage.
- **`state.rs` & `nix_module.rs`:** Persistent `nixapps-state.json` + deterministic `.nix` module generator for both **Home Manager** and **NixOS System** (standard packages + per-app custom Nix blocks), unified diff preview (`similar`), unmanaged-file overwrite protection (`MANAGED_HEADER`), atomic temp-file writes, and `nix-instantiate --parse` validation.
- **`git.rs` & `lib.rs`:** Git Flake tracking check + automatic `git add -N` helper, unified under `ManagedConfig`.
- **`tests/end_to_end.rs` & `examples/core_cli.rs`:** Full integration test suite + developer CLI tool.

### Verification & Real-World Benchmarks
- `cargo fmt --all -- --check` & `cargo clippy --workspace --all-targets -- -D warnings`: **0 warnings**.
- `cargo test --workspace`: **14/14 passing** (13 unit + 1 full end-to-end integration test).
- **Live Channel Test (`core_cli sync nixos-26.05`):** Streamed and indexed **144,349 real packages** from `channels.nixos.org/nixos-26.05/packages.json.br` with zero parsing errors (~37s in unoptimized debug build).
- **Live FTS5 Search (`core_cli search obsidian`):** **1.04 ms** across 144,349 packages, ranking `obsidian` #1.

### Next Steps
- Begin **Phase 2 (`nixapps-gui`)**: build the native GTK4 + Libadwaita + Relm4 desktop interface wired to `nixapps-core`.

---

## Entry #003 — 2026-11-25 — Phase 2: Native GTK4 / Libadwaita GUI MVP (`nixapps-gui`) Complete

- **Phase:** Phase 2 (Native GUI MVP)
- **Participants:** Human Maintainer (Asaph) + AI Engineering Assistant (Google Antigravity)
- **Status:** Completed

### Objective
Deliver a functional, native Linux desktop GUI in **Rust + GTK4 + Libadwaita (Relm4 0.11)** wired directly to `nixapps-core`, covering onboarding, instant search, package detail view, declarative install/uninstall, per-app custom Nix configuration, and installed app management.

### Deliverables Implemented
- **`crates/nixapps-gui/src/main.rs`:** `RelmApp` bootstrap with application ID `io.github.nixapps.NixApps` and global Libadwaita-harmonious CSS.
- **`crates/nixapps-gui/src/settings.rs`:** Persistent user settings (`~/.config/nixapps/settings.json`) and XDG cache DB path (`~/.cache/nixapps/catalog.db`).
- **`crates/nixapps-gui/src/ui_helpers.rs`:** Custom pill badge styling, monospace Nix code view styling, and freedesktop/GNOME category symbolic icon mapping.
- **`crates/nixapps-gui/src/app.rs`:** Full Relm4 `Component` with:
  - **Setup / Onboarding Tab:** Native GTK `FileDialog` folder picker, Home Manager vs NixOS System selector, channel input, one-click managed module initialization, copyable one-line `imports` snippet, and background-threaded channel sync with live `AdwBanner` progress.
  - **Explore / Search Tab:** Instant SQLite FTS5 search, category dropdown, Unfree toggle, themed `AdwActionRow` results with version/unfree/installed badges and quick Install/Remove actions.
  - **App Detail Page:** Hero header with category icon, badges, homepage/GitHub external link (`gtk::UriLauncher`), extended description/briefing, package metadata summary, and a **Declarative Custom Nix Configuration** editor (`Package`, `Package + Custom Nix`, `Custom Nix Only`) validated via `nix-instantiate --parse` with inline syntax error feedback.
  - **Installed Apps Tab:** Managed app list with install mode labels, one-click uninstall, and live generated `.nix` module preview.

### Verification
- `cargo fmt --all -- --check`: **Passed**
- `cargo clippy --workspace --all-targets -- -D warnings`: **0 warnings**
- `cargo test --workspace`: **14/14 passing**
- `cargo build -p nixapps-gui`: **Clean build against GTK 4.24.0 + Libadwaita 1.10.0**

### Next Steps
- Begin **Phase 3 (Rich Store Experience)**: AppStream/Flathub icon & category enrichment, on-demand cached GitHub API briefings, and on-demand local `nix` dependency inspection.

---

## Entry #004 — 2026-11-25 — Phase 3: Rich Store Experience + Git Staging Resilience Fix

- **Phase:** Phase 3 (Rich Store Experience)
- **Participants:** Human Maintainer (Asaph) + AI Engineering Assistant (Google Antigravity)
- **Status:** Completed

### Real-World Bug Caught During Phase 2 Manual Testing
- While testing on `~/av-dotfiles`, `git add -N` failed because `.git/objects/e6` was owned by `root:root` (from a prior `sudo` run), causing `ManagedConfig::apply` to abort package installation.
- **Fix:** Made `ensure_flake_visible` best-effort/non-fatal inside `ManagedConfig::apply`. Managed `.nix` and `nixapps-state.json` writes now always succeed when syntax-valid, surfacing any Git staging issue as a non-fatal `ApplyReport::git_warning` banner/toast.

### Deliverables Implemented
- **`crates/nixapps-core/src/metadata.rs`:** Layered Flathub AppStream API v2 (with curated `attr_path` -> Flathub `app_id` mappings + normalized search matching + Flatpak-disclaimer stripping) and on-demand GitHub REST API (stars, primary language, cleaned Markdown README briefing, owner avatar fallback), backed by 7-day SQLite TTL caching (`metadata_cache`) and local disk icon caching (`~/.cache/nixapps/icons/`).
- **`crates/nixapps-core/src/deps.rs`:** On-demand local Nix dependency evaluator (`inspect_dependencies`) using `NIXPKGS_ALLOW_UNFREE=1 nix eval --impure --json` returning `buildInputs`, `nativeBuildInputs`, and `propagatedBuildInputs`.
- **`crates/nixapps-gui/src/app.rs` & `ui_helpers.rs` & `settings.rs`:**
  - **Store Home View:** Clickable Category tiles with live package counts + **Featured Desktop Apps** card grid with async background icon fetching.
  - **Enriched Detail View:** Real 64px icons, GitHub star badges (`★ 14.2k`), enriched Flathub/GitHub briefings, on-demand **Dependencies** inspector rendered as monospace chips, and 1-click Nix templates (`programs.<name>.enable` and override template).
  - **Setup Tab:** Optional GitHub Personal Access Token field (`adw::PasswordEntryRow`).

### Verification
- `cargo fmt --all -- --check`: **Passed**
- `cargo clippy --workspace --all-targets -- -D warnings`: **0 warnings**
- `cargo test --workspace`: **18/18 passing** (17 unit + 1 end-to-end integration test)
- `cargo build -p nixapps-gui`: **Clean build**

### Next Steps
- Begin **Phase 4 (Rebuild Workflow, Packaging & v1.0 Release)**: Diff preview dialog, configurable Polkit rebuild runner with live logs, Nix Flake `packages.default` + `.desktop` entry, and v1.0.0 release.

---

## Entry #005 — 2026-11-25 — Phase 4: UX Fixes, Parallel Icons, Diff & Polkit Rebuild Runner, Nix Packaging (v1.0.0)

- **Phase:** Phase 4 (Rebuild Workflow, Packaging & v1.0 Release)
- **Participants:** Human Maintainer (Asaph) + AI Engineering Assistant (Google Antigravity)
- **Status:** Completed

### Real-World Feedback Addressed from Phase 3 Testing
1. **Tab Navigation Bug:** Clicking Explore / Installed / Setup in `AdwViewSwitcher` while an app detail page was open did not dismiss the detail view. Fixed by connecting `view_stack.connect_visible_child_name_notify` to automatically close the detail view on tab change.
2. **Icon Fetch Speed:** Replaced sequential 1-by-1 icon downloads (which also downloaded GitHub READMEs) with a pooled keep-alive HTTP client (`OnceLock`), lightweight icon-only lookup (`fetch_icon_cached`), direct GitHub avatar fallback, and a **6-thread concurrent worker pool** in `nixapps-gui`.
3. **Rebuild Ergonomics:** Implemented a queued-changes workflow: users can install/remove multiple apps (writing & validating each change immediately to `nixapps-managed.nix`), review the unified diff in a modal dialog, and click **Apply & Rebuild** once to activate all pending changes with live streaming output.

### Deliverables Implemented
- **`crates/nixapps-core/src/rebuild.rs`:** Auto-detecting Flake/NixOS/Home Manager rebuild command builder (`suggest_rebuild_command`) and live stdout/stderr streaming runner (`run_rebuild_streaming`) supporting Polkit (`pkexec`).
- **`crates/nixapps-core/src/nix_module.rs`:** Exposed `diff_strings` for session-level Diff Preview dialogs.
- **`crates/nixapps-gui/src/app.rs` & `settings.rs`:**
  - **Review Diff Dialog** (`adw::Dialog`) showing unified diff since last rebuild.
  - **Apply & Rebuild Dialog** (`adw::Dialog`) with command preview, live scrolling terminal output, Polkit elevation toggle, and automatic pending-queue reset on success.
  - **Setup Tab Section 4:** Editable rebuild command, Polkit toggle, and 1-click **Auto-Detect Command**.
- **Nix Packaging & Desktop Assets:**
  - `assets/io.github.nixapps.NixApps.desktop` & `assets/io.github.nixapps.NixApps.svg`.
  - Updated `flake.nix` with `packages.default` (`nixapps-1.0.0`), `apps.default`, `wrapGAppsHook4`, and desktop/icon installation.
  - Bumped workspace version to `1.0.0`.

### Verification
- `cargo fmt --all -- --check`: **Passed**
- `cargo clippy --workspace --all-targets -- -D warnings`: **0 warnings**
- `cargo test --workspace`: **21/21 passing** (20 unit + 1 end-to-end integration test)
- `cargo build -p nixapps-gui`: **Clean build (`v1.0.0`)**
- `nix flake show`: **Evaluates cleanly** (`packages.x86_64-linux.default`, `apps.default`, `devShells.default`, `formatter`)

### Next Steps
- Run `git commit`, tag `v1.0.0`, and push to `https://github.com/asphzeera/NixApps`.

---

## Entry #006 — 2026-11-26 — Hotfix: `pkexec` `$SHELL` & NixOS `PATH` Inside `nix develop`

- **Phase:** Post-v1.0.0 Maintenance
- **Participants:** Human Maintainer (Asaph) + AI Engineering Assistant (Google Antigravity)
- **Status:** Completed

### Issue Caught in Real-World Testing
- Running **Apply & Rebuild** from inside `nix develop` caused `pkexec` to fail with `The value for the SHELL variable was not found in the /etc/shells file`, because `nix develop` sets `$SHELL` to the devShell's `/nix/store/...-bash-interactive/bin/bash` rather than a system shell listed in `/etc/shells`.

### Fix Implemented
- Added `resolve_valid_pkexec_shell()` in `crates/nixapps-core/src/rebuild.rs` to force `SHELL` to a valid entry from `/etc/shells` (preferring `/bin/sh`) when invoking `pkexec`.
- Exported standard NixOS paths (`/run/wrappers/bin:/run/current-system/sw/bin:/nix/var/nix/profiles/default/bin`) inside the `pkexec /bin/sh -c` wrapper so `nixos-rebuild`, `nix`, and `git` are always resolved after Polkit environment sanitization.
- Added unit test `pkexec_shell_is_listed_in_etc_shells_when_present`.

---

## Entry #007 — 2026-11-26 — Built-in Password Popup, Adwaita Icons, NixOS Branding & About Dialog

- **Phase:** Post-v1.0.0 UX Polish
- **Participants:** Human Maintainer (Asaph) + AI Engineering Assistant (Google Antigravity)
- **Status:** Completed

### Real-World Issues Addressed
1. **Password prompt fell back to terminal:** On minimal Wayland setups like Niri without a running `polkit-gnome-authentication-agent-1`, `pkexec` fell back to terminal prompts. Added a built-in native Libadwaita **Administrator Password Dialog** (`ElevationMethod::BuiltInSudo`) piping the password securely to `sudo -S` stdin without ever logging or storing it, while retaining Polkit (`pkexec`) and `None` as selectable options in Setup.
2. **Missing button/symbolic icons & branding:** On systems without `adwaita-icon-theme` installed globally, GTK4 symbolic icons were missing, and `gtk::Button` `.label()` + `.icon_name()` overwrote each other. Bundled `adwaita-icon-theme`, `hicolor-icon-theme`, and `librsvg` in `flake.nix`, switched all buttons to `adw::ButtonContent`, added an embedded stylized **NixOS snowflake SVG** to the Setup hero, and added a branded **About NixApps** dialog with the embedded NixApps SVG logo.
