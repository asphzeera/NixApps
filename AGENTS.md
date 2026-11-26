# AGENTS.md — Guidelines for AI Coding Agents

Welcome! **NixApps** is designed to be friendly to both human developers and AI coding agents. If you are an AI agent working in this repository, follow these rules strictly.

---

## 1. Read Before You Write

Before making architectural or non-trivial changes, read:
1. [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) — crate boundaries, data sources, and managed `.nix` module format.
2. [`docs/adr/README.md`](docs/adr/README.md) — existing Architecture Decision Records. **Do not reverse an accepted ADR without creating a new superseding ADR and getting human approval.**
3. [`docs/PRD.md`](docs/PRD.md) — scope and non-goals.
4. [`docs/ROADMAP.md`](docs/ROADMAP.md) — current phase and active milestone.

---

## 2. Core Invariants (Never Violate)

1. **Never edit arbitrary user `.nix` files:** NixApps ONLY reads and writes its own dedicated managed `.nix` module (and its own local state/cache files).
2. **Validate before writing:** Every generated `.nix` file MUST pass `nix-instantiate --parse` (when `nix-instantiate` is available) and be written atomically (write temp file → rename).
3. **No fragile HTML scraping:** Use official `packages.json.br`, `search.nixos.org` API, AppStream/Flathub APIs, on-demand cached GitHub REST API, and local `nix` CLI commands.
4. **Keep `nixapps-core` UI-agnostic:** `crates/nixapps-core` must NEVER depend on `gtk4`, `libadwaita`, or `relm4`. All business logic lives in `nixapps-core` and must be unit-testable headless.
5. **Respect low-RAM Linux desktops:** Avoid unnecessary background polling, unbounded in-memory caches, or heavy dependencies. Stream/index large JSON files directly into SQLite.
6. **Flake + Git awareness:** When creating a new managed `.nix` file inside a Git repository, ensure the user/core handles `git add -N` so Nix Flakes can evaluate it.

---

## 3. Coding & Repository Conventions

- **Language & Stack:** Rust (edition 2024), GTK4 + Libadwaita via Relm4 (`crates/nixapps-gui`), SQLite FTS5 (`crates/nixapps-core`).
- **Formatting & Linting:** Run `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo test --workspace`.
- **Nix Formatting:** Format `flake.nix` and any `.nix` files with `nixfmt` (`nix fmt`).
- **Commits:** Follow [Conventional Commits](https://www.conventionalcommits.org/) (`feat:`, `fix:`, `docs:`, `refactor:`, `test:`, `chore:`).
- **Project Log:** After completing a milestone or significant feature, append a concise entry to [`docs/PROJECT_LOG.md`](docs/PROJECT_LOG.md) and update [`CHANGELOG.md`](CHANGELOG.md) under `[Unreleased]`.

---

## 4. Reporting Changes

When summarizing work for the human maintainer:
- State clearly which files were created or modified.
- List the exact verification commands run (`cargo test`, `clippy`, `nix flake check`, manual GUI test) and their results.
- Flag any assumptions, rate-limit considerations, or open decisions explicitly.
