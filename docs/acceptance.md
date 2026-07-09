# 受け入れチェックリスト

日付: 2026-07-09
仕様の正: `docs/spec.md` §13
確認対象成果物: `src-tauri/target/x86_64-pc-windows-msvc/release/bundle/nsis/Sokki_0.0.0_x64-setup.exe` (25,619,629 bytes)

このファイルは、現時点の受け入れ確認状況を記録する。Windows デスクトップ、WebView2、native dialog、マイク、WASAPI loopback、音声再生、インストーラー実行が必要な項目は、Windows 10/11 x64 実機で確認するまで未完了として扱う。手順は `docs/manual-windows-checks.md` に従う。

## 概要

- 確認済み: WSL2 上の build / typecheck / Rust format / xwin check / xwin clippy / Windows-target test build は通過。
- 確認済み: CPU 版 NSIS installer のクロスビルドは成功。
- 確認済み: GPU クロスビルドは MVP の必須受け入れゲートではなく、任意検証であることを README に記載済み。
- Windows未確認: デバイス、WebView、音声再生、インストーラー、native dialog、リアルタイム挙動は Windows 実機確認待ち。
- タグ状態: Windows manual acceptance が未完了のため、`v1.0.0` タグは未作成。

## WSL 確認済み

- `pnpm build`: PASS
- `pnpm typecheck`: PASS
- `cd src-tauri && cargo fmt --all --check`: PASS
- `cd src-tauri && cargo xwin check --target x86_64-pc-windows-msvc`: PASS
- `cd src-tauri && cargo xwin clippy --target x86_64-pc-windows-msvc --all-targets -- -D warnings`: PASS
- `cd src-tauri && cargo xwin test --target x86_64-pc-windows-msvc --all-targets --no-run`: PASS
- `pnpm tauri:build:win`: PASS
- lockfile はコミット済み: `src-tauri/Cargo.lock`, `pnpm-lock.yaml`

## 録音・音声

- [ ] Windows未確認: マイクのみ / システム音声のみ / 両方 の3構成で録音・再生できる
- [ ] Windows未確認: システム音声のみ録音で、YouTube/Zoom/Teams/ブラウザ音声のいずれかを5秒テスト録音できる
- [ ] Windows未確認: ミックス時、片方が無音でももう片方の録音が継続する
- [ ] Windows未確認: 保存デバイス消失時、候補一覧付きの分かりやすいエラーが出る
- [ ] Windows未確認: 音声テストで無音ソース警告が出て、テスト音声を再生確認できる
- [ ] Windows未確認: 通常負荷でオーディオドロップが発生しない(drop_count=0)

## 文字起こし

- [ ] Windows未確認: 録音中、発話から数秒以内にセグメントが逐次表示される
- [ ] Windows未確認: バッチ処理実行中に録音を開始すると、実行中チャンクが中断(または15秒チャンク1回分以内の待ち)され、rtが優先処理される
- [ ] Windows未確認: バッチ処理は最大15秒チャンクに分割され、キャンセルがチャンク境界で効く
- [ ] Windows未確認: rt強制確定(8秒)およびbatch強制カットのオーバーラップ部で、同一発話の二重セグメントが発生しない
- [ ] Windows未確認: 連続発話中でも、最初のセグメントが8秒+推論時間以内に表示される
- [ ] Windows未確認: `stop_recording` 後、UIは即詳細画面へ遷移し、残処理は `transcribing` → `done` と遷移する(pending_job_count による判定)
- [ ] Windows未確認: mp3 インポートが進捗表示付きで完了し、複数ファイル時に失敗ファイルが理由付きで表示される
- [ ] Windows未確認: `gpu_mode=force_cpu` で必ずCPU動作し、`auto` 失敗時に理由が設定画面に表示される

## モデル

- [ ] Windows未確認: DL完了時にSHA-256検証され、破損は corrupted 表示で再DL誘導される
- [ ] Windows未確認: 未検証(origin=app)モデルは警告付きで使用でき、手動配置モデルは `verify_model` 成功まで使用できない
- [ ] Windows未確認: DLキャンセルで `.part` が残らない

## データ整合性・エクスポート

- [ ] Windows未確認: srt が VLC 等で読み込める
- [ ] Windows未確認: 3時間録音の WAV duration / segments / SRT 時刻が一致する(±100ms)
- [ ] Windows未確認: 録音WAV・テストWAVが WebView から再生できる(M3再生ゲート)

## 復旧・堅牢性

- [ ] Windows未確認: 録音中強制終了→再起動で interrupted+WAV修復され、再文字起こしできる
- [ ] Windows未確認: transcribing中強制終了→再起動で interrupted になり、詳細画面から再文字起こしできる
- [ ] Windows未確認: モデル未DL・デバイス切断・重複起動・ディスクフル(可能なら)でクラッシュせず案内が出る
- [x] WSL確認済み: `pnpm tauri:build:win` による CPU版NSISクロスビルドが Vulkan SDK なしで成功し、Cargo.lock / pnpm-lock.yaml がコミットされている
- [x] docs確認済み: GPUクロスビルドは任意検証であり、MVP受け入れゲートではないことが README に記載されている

## Windows 引き継ぎメモ

`v1.0.0` をタグ付けする前に、上記の成果物またはより新しいリリース候補に対して `docs/manual-windows-checks.md` の手順を Windows 10/11 x64 実機で実行すること。確認後、このファイルに Windows バージョン、確認した成果物名、各項目の pass/fail メモを記録する。
