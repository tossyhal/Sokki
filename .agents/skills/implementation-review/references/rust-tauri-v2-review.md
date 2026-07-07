# Rust / Tauri v2 Review Reference

Use for Rust backend, Tauri commands, async workers, filesystem, config, and desktop integration changes.

## Review Checklist

- Commands expose stable serde-compatible request/response/error shapes.
- Shared state locking cannot deadlock and does not hold locks across blocking or long async work.
- Background workers always decrement counters or release resources on success, error, cancellation, and early return.
- File paths are generated and validated on the Rust side when required by project policy.
- Tauri v2 APIs and capability files are used correctly; no Tauri v1 allowlist/API drift.
- Feature flags keep default builds working without optional SDKs.
- Startup recovery and partial-write handling are considered for persistent state.
- Logs avoid secrets and high-frequency hot paths.
