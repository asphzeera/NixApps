# Roadmap — NixApps

This roadmap tracks NixApps from initial repository foundation through v1.0 release.

---

## Phase 0 — Foundation & Governance ✅ *(Completed)*
- [x] Feasibility study and architecture design
- [x] Initialize Git repository and Rust Cargo workspace (`nixapps-core`, `nixapps-gui`)
- [x] Create Nix Flake (`flake.nix`) with Rust + GTK4 + Libadwaita + SQLite `devShell`
- [x] Write core documentation (`README.md`, `README.pt-BR.md`, `PRD.md`, `ARCHITECTURE.md`, `PROJECT_LOG.md`, `CHANGELOG.md`)
- [x] Write AI transparency & governance docs (`AI_TRANSPARENCY.md`, `AGENTS.md`, `CONTRIBUTING.md`, ADRs `0001`–`0004`)
- [x] Set up GitHub issue/PR templates and CI workflow

---

## Phase 1 — Core Engine (`nixapps-core`) ✅ *(Completed)*
- [x] Design SQLite + FTS5 schema for package catalog, categories, and metadata cache
- [x] Implement streaming downloader & indexer for `channels.nixos.org/<channel>/packages.json.br`
- [x] Implement instant full-text search & filtering API
- [x] Implement managed state model (`nixapps-state.json`) supporting **Home Manager** and **NixOS System**
- [x] Implement deterministic `.nix` module generator (standard packages + custom per-app Nix blocks)
- [x] Implement syntax validation via `nix-instantiate --parse` and atomic writes
- [x] Implement Git flake detection + `git add -N` helper
- [x] Comprehensive unit & integration test suite in `nixapps-core`

---

## Phase 2 — Native GUI MVP (`nixapps-gui` with GTK4/Libadwaita + Relm4) ✅ *(Completed)*
- [x] Bootstrap Relm4 + Libadwaita application window (`AdwApplicationWindow`, `AdwViewSwitcher` / sidebar navigation)
- [x] Onboarding wizard: pick config folder, choose Home Manager vs NixOS System, generate module, show one-line `imports` snippet
- [x] Channel sync progress indicator and status feedback
- [x] Search view with instant results (name, version, short description, installed badge)
- [x] App detail view (name, version, license, unfree badge, homepage/GitHub link, description, Install/Uninstall button)
- [x] Installed Apps tab to list and remove managed packages

---

## Phase 3 — Rich Store Experience ✅ *(Completed)*
- [x] AppStream / Flathub API integration for high-res icons and store categories
- [x] Explore / Categories home view (Development, Games, Graphics, Audio/Video, Office, Utilities, etc.)
- [x] On-demand GitHub API integration (with SQLite TTL cache + optional token setting) for enriched briefings
- [x] On-demand Dependencies viewer (`nix eval` / `nix derivation show`)
- [x] Per-app Custom Configuration editor (enable `programs.*` or custom Nix snippet with live syntax validation & quick templates)

---

## Phase 4 — Rebuild Workflow, Packaging & v1.0 Release ✅ *(Completed)*
- [x] Fix tab switching over App Detail view + 6-thread parallel icon fetching with pooled HTTP client
- [x] Diff preview dialog before/after modifying the managed `.nix` module
- [x] Configurable Rebuild Runner (`nixos-rebuild switch` / `home-manager switch`) via Polkit (`pkexec`) with live streaming log output
- [x] Nix Flake `packages.default`, `apps.default`, desktop entry (`.desktop`), and scalable SVG app icon
- [x] Release **v1.0.0** ready for GitHub publication
