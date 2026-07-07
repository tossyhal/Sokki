---
name: release-packaging
description: Use for release preparation, desktop packaging, installer verification, versioning, build artifacts, Tauri bundle configuration, NSIS, CPU/GPU build variants, release notes, acceptance checklists, and final pre-release validation.
---

# Release Packaging

Use this skill when moving from implementation to distributable artifacts. Prefer documented project commands and record exact versions and artifacts.

## Workflow

1. Read release/build instructions and packaging config before running commands.
2. Confirm version, product identifier, bundle targets, icons/assets, and platform assumptions.
3. Run normal quality gates before packaging.
4. Build the default artifact first; build optional feature variants only when documented or requested.
5. Verify artifact existence, names, sizes, and expected installer/bundle target.
6. Smoke test install/launch when the environment supports it.
7. Update release docs, known limitations, and acceptance records when the release process changes.

## Tauri Desktop Notes

- Confirm CPU/default builds do not require optional GPU SDKs unless explicitly intended.
- Verify NSIS/current-user installer settings when targeting Windows installers.
- Treat audio devices, GPU, local media playback, and asset protocol as real-device checks, not packaging-only checks.
- Keep lockfiles and build config changes in the same release commit when the project requires it.

## Reporting

Report commands, artifacts, environment, skipped smoke tests, and release-blocking risks.
