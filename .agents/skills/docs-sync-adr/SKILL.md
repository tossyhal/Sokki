---
name: docs-sync-adr
description: Use when implementation changes affect documentation, specifications, setup instructions, architecture decisions, dependency versions, public behavior, release notes, acceptance checklists, or when a non-obvious technical decision should be captured as an ADR.
---

# Docs Sync ADR

Use this skill to keep code, specs, and operational docs from drifting. Documentation changes should explain durable behavior, not narrate every implementation step.

## Workflow

1. Identify the source of truth for the area changed.
2. Compare the code change against README, specs, agent instructions, setup docs, acceptance checklists, and release notes.
3. Update docs only where behavior, commands, constraints, dependencies, or user-visible workflows changed.
4. Create or update an ADR when the decision is durable, non-obvious, costly to reverse, or resolves a tradeoff.
5. Cross-link docs instead of duplicating long details.
6. Verify links and paths after moving or renaming docs.

## ADR Guidance

An ADR should include: title, status, date, context, decision, consequences, and alternatives considered. Keep it short and factual.

Do not create an ADR for routine implementation details, cosmetic tweaks, or decisions already mandated by an authoritative spec.

## Sync Triggers

- Dependency version changes or new native/toolchain requirements.
- Public API, command, event, schema, or file format changes.
- Security, privacy, filesystem, or network behavior changes.
- Build, packaging, release, or developer command changes.
- Acceptance criteria status changes.
