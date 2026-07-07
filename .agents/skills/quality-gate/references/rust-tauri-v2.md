# Rust / Tauri v2 Quality Reference

Use this reference when validating Rust backend, Tauri commands, app configuration, capabilities, packaging, or Windows-specific desktop behavior.

## Checks

- Prefer `cargo fmt --all --check` for non-mutating validation; use `cargo fmt --all` only when editing is intended.
- Run `cargo clippy --all-targets -- -D warnings` for substantive Rust changes.
- Run `cargo check` after dependency, feature, command, or module changes.
- Run targeted Rust tests when the crate already has tests or the change adds behavior suitable for tests.
- For Tauri command type changes, also run frontend build/type checks because invoke wrappers can drift.

## Review Points

- Use Tauri v2 APIs and capability files; do not use v1 `tauri::api::*` or allowlist patterns.
- Keep filesystem operations in Rust commands when project policy requires it.
- Return serializable error shapes from commands; do not leak debug-only errors to UI contracts.
- Treat Windows-only code as first-class when the project targets Windows; avoid adding unused cross-platform abstraction.
- Verify asset protocol scope when audio, images, or local files are consumed by WebView.
