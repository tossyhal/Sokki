# Windows Manual Checks

This file records checks that cannot be proven from WSL-only builds/tests. Run these on Windows 10/11 x64 after producing a Windows artifact.

## Policy

- WSL may be used for `pnpm build`, Rust formatting, `cargo xwin check`, `cargo xwin clippy`, and Windows-target test builds.
- WSL may also be used for deterministic Rust/TypeScript logic checks such as JobTracker status transitions, stop flush behavior, worker event payloads, and frontend store updates.
- Windows-target test builds from WSL prove compilation only; they do not prove the generated `.exe` behavior because the test binaries are not executed in WSL.
- Do not treat WSL `tauri dev` or Linux Tauri builds as acceptance evidence.
- Mark each item with the artifact/version tested, Windows version, and result before release.

## WSL Substitute Checks

These checks can be run from WSL before Windows handoff:

- `pnpm build` for TypeScript/Vite production build.
- `cd src-tauri && cargo fmt --all --check`.
- `cd src-tauri && cargo xwin check --target x86_64-pc-windows-msvc`.
- `cd src-tauri && cargo xwin clippy --target x86_64-pc-windows-msvc --all-targets -- -D warnings`.
- `cd src-tauri && cargo xwin test --target x86_64-pc-windows-msvc --all-targets --no-run` to compile Windows-target tests.

For stop/status changes, WSL tests should cover:

- `stop_recording` flushes an active realtime segment and returns without waiting for worker completion.
- The returned session is `transcribing` while pending realtime jobs remain and `done` when none remain.
- JobTracker preserves `error` instead of overwriting it with `done`.
- `session://status` updates list/detail UI state through the session store.

## Export Dialog

Prerequisite: at least one session has transcript segments.

- Open a session detail page and confirm the `エクスポート` button is disabled when there are no segments and enabled when segments exist.
- Click `エクスポート`, choose `TXT`, `SRT`, and `MD` in separate runs, and confirm the native Windows save dialog opens.
- Save each format to a user-selected folder outside the app data directory.
- Confirm the exported file is UTF-8 without BOM and uses LF line endings.
- Confirm TXT joins segment text by newline.
- Confirm SRT contains numbered cues and `HH:MM:SS,mmm --> HH:MM:SS,mmm` timecodes, then open it in VLC or another SRT-capable player.
- Confirm MD contains title, created time, duration, model, language, and timestamped transcript lines.
- After export, click `保存先を開く` and confirm Explorer opens/reveals the exported file.
- Cancel the native save dialog and confirm no file is written and the dialog remains usable.
- Try exporting to an unwritable location and confirm an error is shown without crashing the app.

## Realtime Transcription Pipeline

Prerequisite: a usable local Whisper model is available.

- Start a microphone recording and speak continuously for at least 10 seconds.
- Confirm the session detail transcript shows the live badge and pending row while recording/transcribing.
- Confirm the first realtime transcript segment appears within 8 seconds plus inference time.
- Confirm new realtime segments auto-scroll into view without obscuring the playback controls or header actions.
- Confirm the segment around the 8-second boundary does not duplicate text from the 600ms overlap.
- Start an import/batch transcription, then start recording while it is pending; confirm realtime recording is prioritized and the batch work resumes after recording stops.
- Confirm the recorded WAV duration matches the recording length and the realtime transcript timestamps stay within the recorded duration.
- Stop recording while realtime jobs are still pending and confirm the UI immediately moves to the session detail view without waiting for Whisper completion.
- Confirm the session status is `transcribing` until pending realtime jobs finish, then changes to `done` through `session://status`.
- Force or simulate a transcription error and confirm the session remains `error` with a visible message instead of changing to `done`.

## Existing Desktop/Device Checks Still Required

- Launch the generated Windows `.exe`.
- Launch the generated Windows `.exe` twice and confirm the second launch focuses/restores the existing main window instead of opening another app window.
- Install and launch from the NSIS installer.
- Confirm WebView2 loads the UI.
- Confirm microphone recording.
- Confirm WASAPI loopback system-audio recording.
- Confirm mic + system mix recording.
- Confirm sound-check WAV and recording WAV playback through the WebView asset protocol.
- Confirm `convertFileSrc` playback paths work after app restart.
- Confirm `transcribing` sessions interrupted by app shutdown become `interrupted` on next launch and can be retranscribed.
