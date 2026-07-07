# Dependency / Supply Chain Audit Reference

Use before adding, upgrading, or replacing npm packages, Rust crates, native libraries, or toolchain plugins.

## Minimum Audit

- Confirm the dependency solves a real need that existing project dependencies cannot reasonably cover.
- Prefer established, maintained packages with clear repository, release history, and license.
- Check whether it introduces network calls, telemetry, native code, postinstall scripts, build scripts, or binary downloads.
- Respect project-pinned versions and documented dependency policy; do not upgrade unrelated packages.
- Update and commit lockfiles with manifest changes.
- Verify license compatibility for release-bound software.
- Run relevant tests/builds after dependency changes.

## Rust Crates

- Inspect features; disable defaults if they pull unnecessary backends.
- Watch for `build.rs`, native system dependencies, OpenSSL, GPU SDKs, platform-specific assumptions, and MSVC/Windows support.
- Keep optional heavy integrations behind features when appropriate.

## npm Packages

- Inspect scripts, transitive install behavior, ESM/CJS compatibility, browser vs Node target, and bundle impact.
- Prefer dev dependencies for build/test-only tools.
- Avoid packages that duplicate framework capabilities without clear benefit.

## Reporting

State why the dependency is needed, what version was chosen, lockfile status, and any residual risk.
