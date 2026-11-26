# Product Requirements Document (PRD) — NixApps

- **Status:** Draft v1.0 (Phase 0)
- **Last Updated:** 2026-11-25
- **License:** MIT

---

## 1. Problem Statement

NixOS offers unmatched reproducibility through declarative configuration (especially with **Flakes** and **Home Manager**), but discovering applications, inspecting their metadata/briefings, and adding/removing them still requires switching between a browser (`search.nixos.org`), GitHub repositories, and manual text editing.

Existing graphical attempts often fail on real-world dotfiles because they try to parse and mutate hand-written `configuration.nix` files in place.

---

## 2. Product Vision

**NixApps** is a fast, lightweight, native Linux desktop app store for **NixOS and Home Manager** that bridges graphical software discovery with clean, 100% declarative Nix configuration management.

---

## 3. Target Audience

- NixOS desktop users using **Flakes + Home Manager** or standalone **NixOS system configs**.
- Users who want a familiar app-store experience (browse, categories, search, app briefing, install/uninstall) without abandoning declarative reproducibility.
- Linux desktop users running lightweight Wayland compositors (e.g., Niri, Hyprland, Sway) or GNOME, including machines with limited RAM (8 GB).

---

## 4. Functional Requirements

### 4.1. Onboarding & Configuration Binding
- Select the user's Nix configuration directory (e.g., `~/av-dotfiles` or `/etc/nixos`).
- Choose target mode: **Home Manager** (`home.packages` / `programs.*`) or **NixOS System** (`environment.systemPackages` / `programs.*`) — supporting switching or managing profiles cleanly.
- Create/bind a dedicated managed Nix module file (e.g., `modules/nixapps-managed.nix`).
- Display exact copy-paste instructions showing the one-line `imports = [ ./modules/nixapps-managed.nix ];` snippet.
- Detect Git-tracked Flake repositories and handle staging (`git add -N`) so `nixos-rebuild --flake` recognizes newly created files.

### 4.2. Store Discovery & Search
- **Instant Local Search:** Search nixpkgs packages by name, attribute path, or description via local SQLite FTS5 index synced from official NixOS channel metadata (`packages.json.br`).
- **Categories:** Browse software by standard desktop categories (Development, Graphics, Audio & Video, Games, Office, Network, System, CLI/Utilities, etc.).
- **Channel Selection:** Allow selecting the nixpkgs channel matching the user's system (e.g., `nixos-26.05`, `nixos-unstable`).

### 4.3. Application Detail Page
- Display application icon (AppStream/Flathub with clean category fallback), name, attribute path, version, license, unfree badge, and homepage/GitHub link.
- Display a clean **briefing** combining nixpkgs `longDescription`, AppStream summary, and on-demand cached GitHub repository summary/README excerpt.
- Optional **Dependencies** view queried on demand via local `nix` tooling (`nix eval` / `nix derivation show`).
- **Install / Uninstall** primary action button.
- **Custom Configuration Panel:** Allow the user to attach optional per-app Nix configuration (e.g., enabling a `programs.<name>` module, overrides, or custom Nix snippets recommended in the app's GitHub docs).

### 4.4. Installed Apps Management
- Dedicated **Installed** tab listing all applications managed by NixApps.
- Edit custom per-app configuration or uninstall with one click.
- Preview generated `.nix` diff before/after changes.
- Optional **Apply Changes** action running a user-configured rebuild command (e.g., `nixos-rebuild switch --flake ...` or `home-manager switch`) via Polkit with live log output.

---

## 5. Non-Functional Requirements

- **Safety First:** Never modify any user `.nix` file outside the dedicated managed module. Always validate generated Nix syntax (`nix-instantiate --parse`) before atomic file writes.
- **Low Memory Footprint:** Native Rust + GTK4 / Libadwaita (Relm4), targeting < 80 MB RAM in normal usage.
- **Native Wayland Support:** Clean rendering and theming on Wayland compositors (Niri, GNOME, Hyprland, Sway).
- **Offline-Friendly:** Package search and installed-app management work offline once synced.

---

## 6. Explicit Non-Goals (v1.0)

- Imperative package installs via `nix-env -iA` (strictly declarative by design).
- In-place AST rewriting of arbitrary hand-crafted `configuration.nix` or `flake.nix` files.
- Full flake input management / lockfile editing (deferred to post-v1.0 evaluation).
- Bundling screenshots carousels on v1 (text briefing + icons take priority).
