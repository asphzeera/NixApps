# Changelog

All notable changes to **NixApps** will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- Opaque top-right auto-dismissing notifications (3.5s timer + manual close, left-aligned text) so action feedback never overlaps the opaque bottom-right pending-changes pill.
- Auto-closing rebuild log dialog paired with a **System Updated Successfully!** confirmation popup (with optional **View Log** button) upon clean rebuild completion.
- Floating bottom-right **opaque nested pending-changes pill** (`N pending changes` + inner `Apply & Rebuild` pill button) replacing the top banner for queued installs/removals.
- Accent-styled **declarative Nix template chips** (`Insert programs.<name>.enable` and `Insert override template`) with explanatory hover tooltips.
- Built-in native Libadwaita **Administrator Password Dialog** (`ElevationMethod::BuiltInSudo` via `sudo -S` stdin pipe) so **Apply & Rebuild** opens a graphical password prompt on every Wayland compositor (Niri, Hyprland, Sway, GNOME, KDE) even without a running Polkit agent.
- Branded **About NixApps** dialog (`help-about-symbolic`) with embedded NixApps SVG logo, version, GitHub link, and AI transparency summary.
- Embedded stylized **NixOS snowflake SVG** (scaled to 192×192 vector `GtkPicture`) in the Setup hero.
- Added `adwaita-icon-theme`, `hicolor-icon-theme`, `librsvg`, and `gsettings-desktop-schemas` to `flake.nix` (`packages.default` and `devShells.default`).
- Improved rebuild command auto-detection (`suggest_rebuild_command`) to distinguish standalone Home Manager flakes (`homeConfigurations`) from NixOS system flakes (`nixosConfigurations`).

### Fixed
- Replaced the translucent native GTK tooltip on declarative Nix template chips with a 100% opaque, left-aligned floating `GtkPopover` (`EventControllerMotion`) so hovering never shifts layout or adds scrollbars, and made global tooltips opaque.
- Escaped all Pango markup strings (`&amp;`, `&lt;`, `&gt;`, `markup_escape_text`) across `AdwPreferencesGroup` titles and descriptions to eliminate terminal `Gtk-WARNING` spam on startup.
- Defaulted `GSK_RENDERER=gl` in `flake.nix` (package wrapper and `devShell`) to avoid Vulkan `VK_SUBOPTIMAL_KHR` and deprecated `ngl` warnings on GTK 4.24+ / Wayland compositors like Niri.
- Made the generated `nixapps-managed.nix` module **auto-adaptive** (`config = lib.mkMerge` guarded by `options ? home && options.home ? packages` and `options ? environment && options.environment ? systemPackages`), preventing `option 'home-manager.users.<user>.environment' does not exist` errors regardless of whether the file is imported in `home.nix` or `configuration.nix`.
- Replaced the nested-scroll `AdwStatusPage` in the Setup tab with a clean, non-scrollable responsive header box so the NixOS snowflake logo and description are never clipped or obstructed.
- Upgraded all action buttons (Install, Remove/Uninstall, Apply & Rebuild, Review Diff, Homepage, Copy, Sync, Inspect Dependencies) to use `adw::ButtonContent` so icons and labels render side by side in GTK4.
- Fixed `pkexec` failing inside `nix develop` / Nix shells with `The value for the SHELL variable was not found in the /etc/shells file` by resolving a valid shell from `/etc/shells`, detaching stdin, and preserving standard NixOS system `PATH` entries during elevated rebuilds.

## [1.0.0] - 2026-11-25

### Added
- **Phase 0 — Project Foundation & Governance:**
  - Initialized Cargo workspace with `nixapps-core` (UI-agnostic engine) and `nixapps-gui` (GTK4/Libadwaita/Relm4 entrypoint).
  - Created Nix `flake.nix` with reproducible Rust + GTK4 + Libadwaita + SQLite development shell (`nix develop`).
  - Added open-source documentation: `README.md`, `README.pt-BR.md`, `LICENSE` (MIT), `CONTRIBUTING.md`, `CODE_OF_CONDUCT.md`.
  - Added AI transparency and governance documentation: `AI_TRANSPARENCY.md`, `AGENTS.md`, `docs/PRD.md`, `docs/ARCHITECTURE.md`, `docs/ROADMAP.md`, `docs/PROJECT_LOG.md`, and ADRs `0001`–`0004`.
  - Added GitHub issue templates, PR template (with AI usage disclosure), and CI workflow (`.github/workflows/ci.yml`).
- **Phase 1 — Core Engine (`nixapps-core`):**
  - Implemented SQLite WAL + FTS5 local catalog (`CatalogDb`) with exact/prefix + BM25 ranking and metadata cache table.
  - Implemented low-memory streaming Brotli + JSON syncer (`sync_channel`) verified against 144,349 real packages on `nixos-26.05` (~1 ms search latency).
  - Implemented `ManagedState` (`nixapps-state.json`), deterministic `.nix` generator (`render_nix_module`) for Home Manager and NixOS System, unified diff preview, `nix-instantiate --parse` validation, and atomic file writes with unmanaged-file protection.
  - Implemented Git Flake detection and `git add -N` helper (`ensure_flake_visible`), high-level `ManagedConfig` coordinator, developer CLI example (`core_cli`), and unit/integration test suite.
- **Phase 2 — Native GUI MVP (`nixapps-gui`):**
  - Built native GTK4 + Libadwaita + Relm4 desktop application (`io.github.nixapps.NixApps`).
  - Added Setup/Onboarding view with folder picker, Home Manager vs NixOS System target switch, channel sync with background worker progress banner, and copyable one-line `imports` snippet.
  - Added Explore/Search view with instant FTS5 search, category filter, Unfree toggle, and quick Install/Remove actions.
  - Added Package Detail page with metadata summary, external Homepage/GitHub launcher, and inline `nix-instantiate --parse`-validated Custom Nix configuration editor.
  - Added Installed Apps management tab with live generated `.nix` module preview and `AdwToastOverlay` notifications.
- **Phase 3 — Rich Store Experience:**
  - Added `nixapps_core::metadata` with Flathub AppStream API v2 + on-demand GitHub REST API enrichment, 7-day SQLite TTL caching, and local disk icon caching (`~/.cache/nixapps/icons/`).
  - Added `nixapps_core::deps` for lazy local Nix dependency inspection (`buildInputs`, `nativeBuildInputs`, `propagatedBuildInputs`).
  - Upgraded Explore tab with Store Home view (Category tiles with live counts + Featured Desktop Apps card grid with real icons).
  - Upgraded App Detail view with high-res icons, GitHub star badges, enriched briefings, on-demand Dependencies viewer, and 1-click custom Nix templates.
  - Added optional GitHub Personal Access Token field in Setup settings.
- **Phase 4 — Rebuild Workflow & Nix Packaging:**
  - Added `nixapps_core::rebuild` with Flake/NixOS/Home Manager command auto-detection and live streaming stdout/stderr runner supporting Polkit (`pkexec`).
  - Added **Review Diff** dialog and **Apply & Rebuild** modal dialog with real-time build log streaming and queued pending-changes banner.
  - Added `packages.default`, `apps.default`, freedesktop `.desktop` entry, and scalable SVG icon to `flake.nix`.

### Changed
- Sped up store icon loading significantly using a shared pooled HTTP client (`OnceLock`), lightweight icon-only lookups (skipping README downloads on card grids), direct GitHub avatar fallbacks, and a 6-thread concurrent icon worker pool.

### Fixed
- Switching tabs in the top `AdwViewSwitcher` while an app detail page is open now immediately closes the detail view and displays the chosen tab.
- Made `git add -N` inside `ManagedConfig::apply` non-fatal (`ApplyReport::git_warning`) so `.git/objects` permission mismatches never block saving or installing managed Nix applications.
