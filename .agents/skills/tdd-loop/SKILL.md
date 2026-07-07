---
name: tdd-loop
description: "Use for code changes where tests can clarify or protect behavior: new features, bug fixes, refactors, concurrency-sensitive code, parsers, data models, APIs, Rust modules, TypeScript logic, and regression fixes. Guides Codex through test-first development, minimal implementation, refactoring, and explicit handling of cases that cannot be automated."
---

# TDD Loop

Use this skill to turn a requested change into a tight red-green-refactor loop. Keep the loop proportional: do not invent broad test suites for a tiny edit, but do not skip tests for behavior that can regress.

## Workflow

1. Identify the behavior contract from the user request and repository docs.
2. Inspect existing tests and test helpers before adding new patterns.
3. List the smallest meaningful test cases: happy path, boundary, failure, and regression case if applicable.
4. Add or update a failing test first when the behavior can be automated.
5. Run the narrowest test command that exercises the failure and confirm it fails for the expected reason.
6. Implement the smallest production change that should pass the test.
7. Re-run the narrow test, then add edge cases if the implementation exposed new risk.
8. Refactor only after tests pass, keeping behavior unchanged.
9. Finish with the relevant quality gate for the changed stack.

## When Automation Is Not Enough

- For UI, desktop shell, audio devices, installers, GPU, filesystem permissions, or OS integration, define a manual verification scenario before implementation.
- Record what cannot be automated and why in the final response or the project acceptance document when one exists.
- Prefer deterministic unit tests for pure logic extracted from hard-to-test integrations.

## Test Selection

- Rust: prefer unit tests near pure modules; use integration tests for command, DB, worker, and filesystem behavior.
- TypeScript: prefer component or store tests for UI state; use E2E only for user-visible flows.
- Regression fixes: write a test that fails on the old behavior, not just a broad success test.
- Refactors: first establish characterization tests around externally visible behavior.

## Guardrails

- Do not rewrite large production surfaces before the failing test is understood.
- Do not make tests pass by weakening assertions or hiding errors.
- Do not add sleeps for async behavior unless the project already standardizes on them; prefer events, polling with timeouts, or test doubles.
- Keep fixtures small and local to the behavior under test.
