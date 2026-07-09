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
- `pnpm typecheck` for TypeScript strict type checking.
- `cd src-tauri && cargo fmt --all --check`.
- `cd src-tauri && cargo xwin check --target x86_64-pc-windows-msvc`.
- `cd src-tauri && cargo xwin clippy --target x86_64-pc-windows-msvc --all-targets -- -D warnings`.
- `cd src-tauri && cargo xwin test --target x86_64-pc-windows-msvc --all-targets --no-run` to compile Windows-target tests.
- `pnpm tauri:build:win` to produce the CPU x64 NSIS installer.

Latest WSL-produced artifact:

- `src-tauri/target/x86_64-pc-windows-msvc/release/bundle/nsis/Sokki_0.0.0_x64-setup.exe`
- Size: 25,619,629 bytes
- Result: produced successfully from WSL2 cross build on 2026-07-09.
- Note: this proves packaging only. Launch, installation, WebView2, microphone, WASAPI loopback, mix capture, and asset-protocol playback still require Windows 10/11 x64 manual checks.

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

## Model Management

Prerequisite: network access to Hugging Face is available for model download and verification checks.

- Open Settings and confirm the model list shows all catalog entries with status badges: not downloaded, downloaded, unverified, manual/unverified, or corrupted as applicable.
- Start a model download and confirm `model://progress` updates the progress bar and downloaded byte text.
- Cancel an in-progress download and confirm the progress row clears, the UI remains usable, and a later retry starts from the beginning.
- Complete a model download and confirm the status changes without restarting the app.
- Delete a downloaded model and confirm the row returns to not downloaded.
- Place a model file manually in the app models directory, click verify, and confirm a successful SHA-256 match makes it selectable for recording.
- Attempt verification with a mismatched or truncated model file and confirm the status becomes corrupted and the model is not selectable for recording.
- Confirm the Settings default model selector only enables usable models and shows unverified app-downloaded models as selectable with an unverified label.
- Open the Record page and confirm the model selector disables unusable models and the record button remains disabled when the selected model is not usable.

## Onboarding

Prerequisite: start with a fresh app data directory or set `onboardingDone` to `false` in `settings.json`.

- Launch the app and confirm it redirects to `/onboarding` before the library, record, or settings pages are reachable.
- Step through welcome, model selection, language selection, and finish.
- On the model step, confirm `medium-q5_0` is the default/recommended selection.
- Start a model download and confirm progress bytes update; cancel it and confirm the step remains usable.
- Skip the model download and confirm onboarding can complete, then confirm Settings can be opened afterward.
- Complete onboarding with a downloaded usable model and confirm the Library page opens.
- Restart the app and confirm completed onboarding is not shown again.
- Set `onboardingDone=false` again and confirm direct navigation to `/record` or `/settings` redirects back to onboarding.

## Import Dialog

Prerequisite: at least one usable local Whisper model is available.

- Open Library and click `インポート`; confirm the native Windows open dialog appears with audio file filters.
- Select one valid audio file and confirm an import result row reports success and the Library session list refreshes.
- Select multiple files where at least one is invalid or unsupported and confirm each failed file is shown with its file name, error code, and reason.
- Cancel the native open dialog and confirm no import starts and no stale result is shown.
- Remove or corrupt all usable models and confirm clicking `インポート` shows a settings guidance message instead of opening a broken import flow.
- Confirm the frontend never asks for a save path or constructs an app-data recording path; imported file paths are only passed to the Rust `import_files` command.

## Sound Check And Recording Exclusion

Prerequisite: at least one usable local Whisper model is available.

- Start a sound check and immediately try to start recording before the sound check finishes.
- Confirm recording does not start while the sound check is running and the app shows a `SOUND_CHECK_BUSY` style error instead of creating a session.
- After the sound check finishes, start recording and confirm recording can begin normally.
- While recording is active, try to start a sound check and confirm it is rejected with an already-recording message.

## Realtime Transcription Pipeline

Prerequisite: a usable local Whisper model is available.

- Start a microphone recording and speak continuously for at least 10 seconds.
- Confirm the Record page live transcript panel appends realtime segments while recording, keeps the latest segment in view by default, and shows the `最新へ` control after manually scrolling away from the bottom.
- Confirm the session detail transcript shows the live badge and pending row while recording/transcribing.
- Confirm the first realtime transcript segment appears within 8 seconds plus inference time.
- Confirm new realtime segments auto-scroll into view without obscuring the playback controls or header actions.
- Confirm the segment around the 8-second boundary does not duplicate text from the 600ms overlap.
- Start an import/batch transcription, then start recording while it is pending; confirm realtime recording is prioritized and the batch work resumes after recording stops.
- Confirm the recorded WAV duration matches the recording length and the realtime transcript timestamps stay within the recorded duration.
- Stop recording while realtime jobs are still pending and confirm the UI immediately moves to the session detail view without waiting for Whisper completion.
- Confirm the session status is `transcribing` until pending realtime jobs finish, then changes to `done` through `session://status`.
- Force or simulate a transcription error and confirm the session remains `error` with a visible message instead of changing to `done`.
- For a long-running recording, confirm the 10-minute-before-limit warning appears and the 3-hour limit automatically stops recording without leaving a second active recording state.

## Existing Desktop/Device Checks Still Required

- Launch the generated Windows `.exe`.
- Launch the generated Windows `.exe` twice and confirm the second launch focuses/restores the existing main window instead of opening another app window.
- Install and launch from the NSIS installer.
- Confirm WebView2 loads the UI.
- Confirm microphone recording.
- Confirm WASAPI loopback system-audio recording.
- Confirm mic + system mix recording.
- During an active microphone recording, unplug or disable the selected input device and confirm recording auto-stops, the WAV remains playable, and the session becomes `error` with a `DEVICE_LOST` message.
- During an active system-audio or mix recording, disable the selected output device and confirm the same `DEVICE_LOST` auto-stop behavior.
- Confirm sound-check WAV and recording WAV playback through the WebView asset protocol.
- Confirm `convertFileSrc` playback paths work after app restart.
- Confirm `transcribing` sessions interrupted by app shutdown become `interrupted` on next launch and can be retranscribed.
- Confirm `error` and `interrupted` sessions show visible badges/messages in the library and session detail views.
- Confirm deleting from both the library card and session detail opens the in-app confirmation dialog and that canceling leaves the session intact.
- Confirm the record setup view shows a clear warning when the selected source requires a missing input or output device.
- Confirm the settings screen shows the CPU build note, disables GPU-using mode choices, and updates backend labels after changing GPU mode.
- Confirm realtime transcript segments still appear once in the session detail view after navigating away and back.
