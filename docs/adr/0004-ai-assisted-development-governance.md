# ADR-0004: Transparent Human-in-the-Loop AI-Assisted Open-Source Governance

- **Status:** Accepted
- **Date:** 2026-11-25

## Context

NixApps is being designed and built collaboratively by a human maintainer and AI coding agents. Open-source communities increasingly value honesty regarding AI involvement, reproducibility of architectural intent, and clear safety boundaries for automated contributors.

## Decision

1. Explicitly disclose AI-assisted engineering in `README.md`, `AI_TRANSPARENCY.md`, and the GitHub PR template.
2. Maintain `AGENTS.md` with non-negotiable engineering invariants so both human and AI contributors adhere to safety and architecture rules.
3. Track every phase and milestone in `docs/PROJECT_LOG.md`, `CHANGELOG.md`, and formal ADRs under `docs/adr/`.
4. Release under the permissive **MIT License** to encourage widespread use, forks, and community improvements.

## Consequences

- **Positive:** High trust and auditability; easy onboarding for external contributors and AI workflows alike; durable project memory across sessions.
- **Negative:** Slight documentation overhead per milestone (mitigated by keeping logs concise and structured).
