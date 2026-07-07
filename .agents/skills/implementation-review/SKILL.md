---
name: implementation-review
description: Use after implementation or when explicitly asked for a review. Reviews code changes for correctness, regressions, missing tests, race conditions, error handling, UI state gaps, Tauri/Rust/React/TypeScript/Tailwind issues, and divergence from repository specifications.
---

# Implementation Review

Use this skill in a code-review posture. Findings lead; summaries are secondary. Focus on bugs and risks that matter to users or maintainers.

## Workflow

1. Inspect `git diff` and changed files; separate user-existing changes from current work.
2. Read the repository's authoritative specs or docs for the touched area.
3. Load references only when relevant:
   - Rust/Tauri v2: `references/rust-tauri-v2-review.md`
   - React/TypeScript/Tailwind: `references/react-ts-tailwind-review.md`
4. Review behavior, not just style: failure modes, edge cases, concurrency, persistence, security, and UX states.
5. Check whether tests cover the changed contract and important regressions.
6. Report findings by severity with file/line references when available.

## Output Rules

- If issues exist, list findings first in severity order.
- If no issues are found, say so clearly and mention remaining test gaps or residual risk.
- Do not bury a correctness issue in a general summary.
- Do not request broad rewrites unless the current design creates concrete risk.
