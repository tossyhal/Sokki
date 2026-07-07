---
name: agent-safety-and-supply-chain
description: Use when agent work may affect user changes, destructive commands, filesystem boundaries, secrets, network access, dependency additions, lockfiles, package provenance, build scripts, native dependencies, licenses, or supply-chain risk. Also use before adding npm/crates dependencies or running risky automation.
---

# Agent Safety And Supply Chain

Use this skill to keep agent actions reversible, scoped, and auditable. For dependency changes, load `references/dependency-audit.md`.

## Agent Safety Workflow

1. Inspect `git status` before edits or commits.
2. Identify user-existing changes and avoid reverting or mixing them unless explicitly requested.
3. Prefer narrow, non-destructive commands. Ask before destructive commands or actions outside the workspace.
4. Treat network access, credential use, installers, migrations, and package downloads as explicit risk points.
5. Do not print secrets, tokens, private keys, or sensitive local paths unless required and approved.
6. If a command fails due to sandbox/network limits and is necessary, rerun with explicit approval rather than working around policy.
7. Report skipped risky actions and any manual approval assumptions.

## Dependency Workflow

Before adding or upgrading dependencies, read `references/dependency-audit.md` and confirm the dependency is necessary, scoped, maintained, compatible with project pins, and reflected in lockfiles.

## Guardrails

- Do not run `git reset --hard`, checkout over user changes, or broad deletion commands without explicit request.
- Do not silently change dependency major versions or toolchain requirements.
- Do not add telemetry, external calls, or postinstall/native build behavior without documenting it.
