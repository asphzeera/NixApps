# ADR-0001: Use a Dedicated Managed `.nix` Module Instead of In-Place AST Editing

- **Status:** Accepted
- **Date:** 2026-11-25

## Context

Nix configuration files are written in a full functional programming language, not a flat key-value format. Real-world NixOS and Home Manager setups rely on `let ... in` bindings, Flakes, overlays, custom overrides, and modular imports. Tools that rewrite `configuration.nix` directly via AST manipulation risk breaking user code, mangling comments, or failing on non-standard patterns.

## Decision

NixApps will **never modify hand-written user `.nix` files**. Instead:
1. NixApps maintains a structured state file (`nixapps-state.json`) representing managed apps and optional per-app custom Nix snippets.
2. From that state, NixApps deterministically generates a single dedicated module (e.g., `modules/nixapps-managed.nix`) targeting either **Home Manager** (`home.packages`) or **NixOS System** (`environment.systemPackages`).
3. The user imports this module with a single line in their existing `home.nix` or `configuration.nix`.
4. Every write is validated with `nix-instantiate --parse` and committed atomically.

## Consequences

- **Positive:** Zero risk of corrupting hand-written user dotfiles; clean diffs in Git; trivial to uninstall NixApps by removing one import line; easy to support custom per-app Nix blocks safely.
- **Negative:** Requires one manual onboarding step (adding the `imports` entry once) and Git flake tracking (`git add -N`) for newly created files inside Git-backed Flake repositories.
