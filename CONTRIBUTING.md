# Contributing to NixApps

Thank you for your interest in contributing to **NixApps**! Whether you write every line by hand or collaborate with AI coding tools, your contributions are welcome.

---

## Ways to Contribute

- **Bug Reports:** Found an issue on your NixOS or Home Manager setup? Open a bug report with your NixOS version, compositor/DE, and reproduction steps.
- **Feature Requests:** Suggest improvements to search, metadata sources, custom module templates, or UI polish.
- **Code & Tests:** Pick an item from [`docs/ROADMAP.md`](docs/ROADMAP.md) or open issues.
- **Metadata & Packaging:** Help improve AppStream/category mapping or Nix Flake packaging.

---

## Development Setup

```bash
git clone https://github.com/asphzeera/NixApps.git
cd NixApps
nix develop
```

Inside the dev shell:

```bash
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

---

## Pull Request Guidelines

1. **Keep changes focused:** One logical feature or fix per PR.
2. **Respect crate boundaries:** Core logic goes in `crates/nixapps-core` with unit tests; UI code goes in `crates/nixapps-gui`.
3. **Update docs when needed:** If you change architecture or add a major decision, add an ADR in `docs/adr/` and update `CHANGELOG.md`.
4. **AI Usage Disclosure:** If you used AI tools to assist with your PR, please note it honestly in the Pull Request template and confirm that you reviewed and tested the generated code.
5. **Commit Messages:** Use [Conventional Commits](https://www.conventionalcommits.org/) (`feat:`, `fix:`, `docs:`, `refactor:`, `test:`, `chore:`).

If you are using an AI coding agent to contribute, please point it to [`AGENTS.md`](AGENTS.md) first!
