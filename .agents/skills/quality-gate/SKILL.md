---
name: quality-gate
description: Use before claiming implementation is complete, before commits or PRs, and after non-trivial edits. Selects and runs the right validation commands for Rust, Tauri v2, React, TypeScript, Tailwind, tests, formatting, linting, builds, and manual verification; records any checks that could not be run.
---

# Quality Gate

Use this skill to choose the minimum sufficient verification set for a change. Run checks from narrow to broad, and report failures with the command and the actionable cause.

## Workflow

1. Inspect the changed files and identify affected stacks.
2. Read stack references only when relevant:
   - Rust/Tauri v2: `references/rust-tauri-v2.md`
   - React/TypeScript/Tailwind: `references/react-ts-tailwind.md`
3. Run the narrowest tests first, then formatting/lint/type checks, then build-level checks.
4. If a check fails, fix the root cause and re-run the failed check before moving on.
5. If a required check cannot run, state the reason and the residual risk.

## Default Check Matrix

- Rust logic change: targeted Rust test if available, `cargo fmt --all --check`, `cargo clippy --all-targets -- -D warnings`, `cargo check`.
- Tauri command/config change: Rust checks plus frontend build if command types or UI calls changed.
- React/TypeScript change: targeted test if available, TypeScript/lint command if present, `npm run build`.
- Tailwind/design change: frontend build plus visual or screenshot check when user-visible.
- Dependency change: install/check lockfile, build, and use `$agent-safety-and-supply-chain` for dependency audit.
- Release change: use `$release-packaging`.

## Reporting

End with a concise list of commands run and their result. Mention skipped checks explicitly; do not imply unrun checks passed.
