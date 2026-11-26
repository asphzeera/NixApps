# AI Transparency Statement

**NixApps** is an open-source project intentionally built using **AI-Assisted Engineering with Human-in-the-Loop governance**. We believe in full radical transparency about how AI is used in software creation so users and contributors can trust, audit, and improve the codebase.

---

## How Human + AI Collaboration Works Here

| Responsibility | Primary Owner | Details |
| :--- | :--- | :--- |
| **Product Vision & UX Direction** | Human Maintainer | Defines goals, evaluates ergonomics on real NixOS desktops, and decides scope/priorities. |
| **Architectural Decisions** | Joint (Human-approved) | AI proposes trade-offs and drafts ADRs; Human reviews and approves every major architectural choice. |
| **Code & Tests** | AI-Assisted / Joint | AI drafts Rust/GTK4 modules, parsers, generators, and test suites; Human reviews, compiles, and verifies behavior. |
| **Safety & Verification** | Automated CI + Human | All Nix output is validated with `nix-instantiate --parse`; Rust code must pass `cargo test`, `clippy`, and `rustfmt`. |
| **Documentation & Project Log** | Joint | Every milestone, decision, and known limitation is logged in `docs/PROJECT_LOG.md` and `docs/adr/`. |

---

## Principles We Follow

1. **No Black Boxes:** AI-generated code must remain clean, idiomatic, modular, and readable by humans.
2. **Never Corrupt User Dotfiles:** Safety invariants (such as managing only a dedicated `.nix` file and validating syntax before atomic writes) are enforced by deterministic code and unit tests — never left to chance.
3. **Explicit Disclosure:** Pull Requests and major milestones document whether AI tools were used and how the changes were verified.
4. **Open to Everyone:** Contributors are welcome whether they write every line by hand or collaborate with AI coding agents (see [`AGENTS.md`](AGENTS.md) and [`CONTRIBUTING.md`](CONTRIBUTING.md)).

---

## Tools Used

- **AI Coding Assistant:** Google Antigravity (Gemini models) for initial architecture, planning, documentation, and implementation assistance.
- **Verification Environment:** Real NixOS 26.05 (Flakes + Home Manager + Wayland/Niri) host and automated GitHub Actions CI.

*(This list will be updated as new tools or contributors join the project.)*
