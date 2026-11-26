# ADR-0002: Use Rust + GTK4 / Libadwaita (Relm4) for Native Desktop GUI

- **Status:** Accepted
- **Date:** 2026-11-25

## Context

NixApps targets Linux/NixOS desktop users, including lightweight Wayland compositors (Niri, Hyprland, Sway) and machines with constrained RAM (e.g., 8 GB RAM with shared iGPU). We evaluated three GUI stacks:
1. Tauri v2 (Rust + WebKitGTK + JS/TS frontend)
2. Python + GTK4 / Libadwaita
3. Rust + GTK4 / Libadwaita via Relm4

## Decision

We selected **Rust + GTK4 / Libadwaita (Relm4)** organized as a Cargo workspace (`nixapps-core` + `nixapps-gui`).

## Consequences

- **Positive:**
  - Minimal memory footprint (~25–50 MB RAM) with zero WebView/Chromium overhead.
  - First-class Wayland support and native Linux desktop integration (GNOME/Libadwaita HIG).
  - Single strongly typed language (Rust) across core indexing, Nix generation, and UI.
  - Fast SQLite FTS5 indexing of 100k+ nixpkgs entries without GIL or IPC serialization overhead.
- **Negative:**
  - Longer compile times during development compared to Python or web frontends.
  - Slightly steeper contribution curve for developers unfamiliar with Rust/GTK4.
