# ADR-0003: Use Official NixOS Channels, AppStream/Flathub, and Cached GitHub API

- **Status:** Accepted
- **Date:** 2026-11-25

## Context

An app store requires fast package search, icons, categories, application briefings, and dependency visibility. HTML web scraping of `search.nixos.org` or GitHub pages is slow, brittle, and violates good API citizenship. Furthermore, `nixpkgs` alone does not ship standardized store categories or icons for every package.

## Decision

NixApps will assemble package metadata through a layered, API-first pipeline with zero HTML scraping:
1. **Catalog & Core Metadata:** Stream official Brotli-compressed `packages.json.br` from `channels.nixos.org` into a local **SQLite FTS5** database for instant offline search.
2. **Icons & Categories:** Match packages against AppStream metadata / Flathub public APIs, cached locally, with clean category-based fallback icons for CLI/library packages.
3. **Briefings:** Combine `meta.description` / `meta.longDescription` with on-demand GitHub REST API lookups (strictly when opening a detail page, cached with TTL to respect rate limits, with optional user PAT support).
4. **Dependencies:** Query lazily via local `nix` CLI (`nix eval` / `nix derivation show`) to reflect the user's actual pinned `nixpkgs`.

## Consequences

- **Positive:** Sub-20ms offline search, resilient to web UI changes, respectful of upstream rate limits, accurate dependency data.
- **Negative:** Initial sync requires downloading ~20–40 MB compressed `packages.json.br`; icon/category coverage for niche CLI tools relies on graceful fallbacks.
