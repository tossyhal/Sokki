# Acceptance Checklist

Date: 2026-07-09
Spec source: `docs/spec.md` §13
Artifact under review: `src-tauri/target/x86_64-pc-windows-msvc/release/bundle/nsis/Sokki_0.0.0_x64-setup.exe` (25,610,394 bytes)

This file records the current verification state. Items that need a Windows desktop, WebView2, native dialogs, microphone, WASAPI loopback, playback, or installer execution are intentionally left pending until they are checked on Windows 10/11 x64. Use `docs/manual-windows-checks.md` for the manual procedures.

## Summary

- PASS: WSL2 build/static gates and CPU NSIS cross build.
- PASS: GPU cross build is documented as optional and non-blocking for MVP.
- PENDING_WINDOWS: Device, WebView, playback, installer, native dialog, and real-time behavior checks.
- Tag status: `v1.0.0` is not tagged yet because Windows manual acceptance is still pending.

## WSL Evidence

- `pnpm build`: PASS
- `cd src-tauri && cargo fmt --all --check`: PASS
- `cd src-tauri && cargo xwin check --target x86_64-pc-windows-msvc`: PASS
- `cd src-tauri && cargo xwin clippy --target x86_64-pc-windows-msvc --all-targets -- -D warnings`: PASS
- `cd src-tauri && cargo xwin test --target x86_64-pc-windows-msvc --all-targets --no-run`: PASS
- `pnpm tauri:build:win`: PASS
- Lockfiles committed: `src-tauri/Cargo.lock`, `pnpm-lock.yaml`

## Recording And Audio

- [ ] PENDING_WINDOWS: マイクのみ / システム音声のみ / 両方 の3構成で録音・再生できる
- [ ] PENDING_WINDOWS: システム音声のみ録音で、YouTube/Zoom/Teams/ブラウザ音声のいずれかを5秒テスト録音できる
- [ ] PENDING_WINDOWS: ミックス時、片方が無音でももう片方の録音が継続する
- [ ] PENDING_WINDOWS: 保存デバイス消失時、候補一覧付きの分かりやすいエラーが出る
- [ ] PENDING_WINDOWS: 音声テストで無音ソース警告が出て、テスト音声を再生確認できる
- [ ] PENDING_WINDOWS: 通常負荷でオーディオドロップが発生しない(drop_count=0)

## Transcription

- [ ] PENDING_WINDOWS: 録音中、発話から数秒以内にセグメントが逐次表示される
- [ ] PENDING_WINDOWS: バッチ処理実行中に録音を開始すると、実行中チャンクが中断(または15秒チャンク1回分以内の待ち)され、rtが優先処理される
- [ ] PENDING_WINDOWS: バッチ処理は最大15秒チャンクに分割され、キャンセルがチャンク境界で効く
- [ ] PENDING_WINDOWS: rt強制確定(8秒)およびbatch強制カットのオーバーラップ部で、同一発話の二重セグメントが発生しない
- [ ] PENDING_WINDOWS: 連続発話中でも、最初のセグメントが8秒+推論時間以内に表示される
- [ ] PENDING_WINDOWS: stop_recording 後、UIは即詳細画面へ遷移し、残処理は transcribing→done と遷移する(pending_job_count による判定)
- [ ] PENDING_WINDOWS: mp3 インポートが進捗表示付きで完了し、複数ファイル時に失敗ファイルが理由付きで表示される
- [ ] PENDING_WINDOWS: gpu_mode=force_cpu で必ずCPU動作、auto失敗時に理由が設定画面に表示される

## Models

- [ ] PENDING_WINDOWS: DL完了時にSHA-256検証され、破損は corrupted 表示で再DL誘導される
- [ ] PENDING_WINDOWS: 未検証(origin=app)モデルは警告付きで使用でき、手動配置モデルは verify_model 成功まで使用できない
- [ ] PENDING_WINDOWS: DLキャンセルで `.part` が残らない

## Data Integrity And Export

- [ ] PENDING_WINDOWS: srt が VLC 等で読み込める
- [ ] PENDING_WINDOWS: 3時間録音の WAV duration / segments / SRT 時刻が一致(±100ms)
- [ ] PENDING_WINDOWS: 録音WAV・テストWAVが WebView から再生できる(M3再生ゲート)

## Recovery And Robustness

- [ ] PENDING_WINDOWS: 録音中強制終了→再起動で interrupted+WAV修復され、再文字起こしできる
- [ ] PENDING_WINDOWS: transcribing中強制終了→再起動で interrupted になり、詳細画面から再文字起こしできる
- [ ] PENDING_WINDOWS: モデル未DL・デバイス切断・重複起動・ディスクフル(可能なら)でクラッシュせず案内が出る
- [x] PASS_WSL: `pnpm tauri:build:win` による CPU版NSISクロスビルドが Vulkan SDK なしで成功し、Cargo.lock / pnpm-lock.yaml がコミットされている
- [x] PASS_DOCS: GPUクロスビルドは任意検証であり、MVP受け入れゲートではないことが README に記載されている

## Windows Handoff Notes

Before tagging `v1.0.0`, run the manual checklist in `docs/manual-windows-checks.md` against the artifact named above or a newer release candidate, then update this file with Windows version, artifact name, and pass/fail notes.
