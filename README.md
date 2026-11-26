# NixApps

> **A native, lightweight, declarative App Store for NixOS and Home Manager — built with Rust, GTK4, Libadwaita, and Relm4.**

[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Built With: AI-Assisted Engineering](https://img.shields.io/badge/Development-AI--Assisted%20%2B%20Human--in--the--Loop-purple.svg)](AI_TRANSPARENCY.md)
[![Stack: Rust + GTK4 + Relm4](https://img.shields.io/badge/Stack-Rust%20%2B%20GTK4%20%2F%20Libadwaita-orange.svg)](docs/ARCHITECTURE.md)
[![Status: v1.0.0 — Ready](https://img.shields.io/badge/Status-v1.0.0%20Ready-brightgreen.svg)](docs/ROADMAP.md)

[🇺🇸 English](README.md) | [🇧🇷 Português (Brasil)](README.pt-BR.md)

---

## 🤖 Transparency Notice: Built with AI

**NixApps is openly built through a Human-in-the-Loop, AI-Assisted Engineering workflow.**

- Every architectural decision, requirement, and real-world test on NixOS is directed and validated by human maintainers.
- AI coding agents assist with architecture drafting, Rust/GTK4 implementation, automated tests, and documentation.
- We document **every major decision** in [`docs/adr/`](docs/adr/README.md), track progress in [`docs/PROJECT_LOG.md`](docs/PROJECT_LOG.md), and provide [`AGENTS.md`](AGENTS.md) so anyone — human or AI-assisted — can audit, use, and improve this project.

Read our full manifesto in **[`AI_TRANSPARENCY.md`](AI_TRANSPARENCY.md)**.

---

## ✨ Why NixApps?

Most graphical software centers for NixOS try to parse and rewrite hand-crafted `configuration.nix` files in place. On real-world Flakes + Home Manager setups (with `let ... in`, overlays, overrides, and split modules), AST editing is fragile.

**NixApps takes a safer, purely declarative approach:**

1. **Dedicated Managed Module:** During onboarding, you point NixApps to your Nix configuration folder and choose your target (**Home Manager** or **NixOS System**).
2. **One-Line Import:** You add a single line (`./modules/nixapps-managed.nix`) to your `home.nix` or `configuration.nix`. NixApps **never touches your hand-written files**.
3. **Custom Per-App Configuration:** Install packages normally *or* attach custom Nix options / `programs.*` / overrides per application right from the app's detail page (validated via `nix-instantiate --parse` before saving).
4. **Queue & Apply with Live Rebuild:** Queue multiple app installs/removals, review the unified `.nix` diff, and click **Apply & Rebuild** to run `nixos-rebuild switch` or `home-manager switch` via Polkit with live log streaming.
5. **Zero Fragile HTML Scraping:** Powered by official NixOS channel metadata (`packages.json.br`) indexed locally in SQLite FTS5 (~1 ms offline search across 144k+ packages), enriched with Flathub AppStream icons, cached GitHub briefings, and on-demand `nix eval` dependency inspection.
6. **100% Native & Lightweight:** Written in **Rust + GTK4 / Libadwaita (Relm4)** — no Electron, no WebView, low RAM footprint, native Wayland support.

---

## 🗺️ Roadmap Status (v1.0.0 Complete)

- [x] **Phase 0:** Repository structure, Nix Flake devShell, Cargo workspace, PRD, Architecture, ADRs, and AI governance docs.
- [x] **Phase 1:** `nixapps-core` engine (streaming `packages.json.br` sync of 144k+ packages, ~1 ms SQLite FTS5 search, deterministic `.nix` module generator + `nix-instantiate --parse` validator + Git flake helper).
- [x] **Phase 2:** `nixapps-gui` MVP (GTK4/Libadwaita onboarding, instant search, package detail view + custom Nix editor, install/uninstall, installed apps manager).
- [x] **Phase 3:** Rich store experience (Store Home with Category tiles & Featured App cards, Flathub/GitHub real icons & cached briefings, on-demand Nix dependency viewer, 1-click custom Nix templates).
- [x] **Phase 4:** Diff preview dialog, configurable Polkit rebuild runner with live streaming logs, 6-thread parallel icon fetching, Flake package + `.desktop` entry + SVG icon (**v1.0.0**).

See [`docs/ROADMAP.md`](docs/ROADMAP.md) and [`docs/PROJECT_LOG.md`](docs/PROJECT_LOG.md) for full details.

---

## 🚀 Try or Install with Nix Flakes

### Run directly
```bash
nix run github:asphzeera/NixApps
```

### Add to your NixOS / Home Manager `flake.nix`
```nix
inputs.nixapps.url = "github:asphzeera/NixApps";
# Then add `inputs.nixapps.packages.${pkgs.stdenv.hostPlatform.system}.default`
# to `home.packages` or `environment.systemPackages`.
```

### Local Development
```bash
git clone https://github.com/asphzeera/NixApps.git
cd NixApps
nix develop
cargo test --workspace
cargo run -p nixapps-gui
```

---

## 📚 Project Documentation

| Document | Purpose |
| :--- | :--- |
| [`docs/PRD.md`](docs/PRD.md) | Product Requirements Document (features, scope, non-goals) |
| [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) | Technical architecture, crate boundaries, metadata pipeline, managed `.nix` format |
| [`docs/ROADMAP.md`](docs/ROADMAP.md) | Milestone breakdown from Phase 0 to v1.0 |
| [`docs/PROJECT_LOG.md`](docs/PROJECT_LOG.md) | Engineering & project management journal |
| [`docs/adr/`](docs/adr/README.md) | Architecture Decision Records (ADRs) |
| [`AI_TRANSPARENCY.md`](AI_TRANSPARENCY.md) | How AI is used in NixApps and quality standards |
| [`AGENTS.md`](AGENTS.md) | Instructions for AI coding agents contributing to this repo |
| [`CONTRIBUTING.md`](CONTRIBUTING.md) | Contribution guidelines for humans and AI-assisted workflows |

---

## 📄 License

Distributed under the **MIT License**. See [`LICENSE`](LICENSE) for details.
