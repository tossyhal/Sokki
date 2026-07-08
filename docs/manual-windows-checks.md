# Windows Manual Checks

This file records checks that cannot be proven from WSL-only builds/tests. Run these on Windows 10/11 x64 after producing a Windows artifact.

## Policy

- WSL may be used for `pnpm build`, Rust formatting, `cargo xwin check`, `cargo xwin clippy`, and Windows-target test builds.
- Do not treat WSL `tauri dev` or Linux Tauri builds as acceptance evidence.
- Mark each item with the artifact/version tested, Windows version, and result before release.

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
- Confirm the first realtime transcript segment appears within 8 seconds plus inference time.
- Confirm the segment around the 8-second boundary does not duplicate text from the 600ms overlap.
- Start an import/batch transcription, then start recording while it is pending; confirm realtime recording is prioritized and the batch work resumes after recording stops.
- Confirm the recorded WAV duration matches the recording length and the realtime transcript timestamps stay within the recorded duration.
- Stop recording while realtime jobs are still pending and confirm the UI remains responsive without waiting for Whisper completion.

## Existing Desktop/Device Checks Still Required

- Launch the generated Windows `.exe`.
- Install and launch from the NSIS installer.
- Confirm WebView2 loads the UI.
- Confirm microphone recording.
- Confirm WASAPI loopback system-audio recording.
- Confirm mic + system mix recording.
- Confirm sound-check WAV and recording WAV playback through the WebView asset protocol.
- Confirm `convertFileSrc` playback paths work after app restart.
- Confirm `transcribing` sessions interrupted by app shutdown become `interrupted` on next launch and can be retranscribed.
