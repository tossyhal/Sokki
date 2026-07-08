# 残タスク一覧(2026-07-09 時点)

作業を中断するにあたり、現状と残タスクをまとめる。実装単位の正は `docs/spec.md` §15 のコミット計画。

## 完了済み(今回のセッション)

- `origin/develop` 時点で `docs/spec.md` §15 の M1、M3、M4、M5 は実装コミットとして完了済み。
  - M3: 音声デバイス列挙、buffer pool、cpal mic / WASAPI loopback、resample、mixer、WAV writer、録音ライフサイクル、録音UI、sound check、起動時復旧、Library / Session detail。
  - M4: JobTracker、Whisper context、worker scheduler、inference params、overlap/duplicate suppression、decode/import/chunking/cancel/retranscribe/export。
  - M5: realtime segmenter、mixer→worker結線、live transcript UI、stop flush / async completion、scheduler priority/preemption test。
- `ff5aa9d`〜`9921b92` で M6 の一部も実装済み。
  - single instance、max recording duration guard、device lost auto stop、empty/error/dialog polish、GPU mode setting、event listener/store update audit。
- `8e62dc1` fix: validate import extension before file io and repair failing tests
  - import の拡張子検証をファイルIOより先に実施(§6.5 の順序)。settings/worker のテスト不備も修復。全120テストがパス。
- `7b9d338` feat: wire capture resample mixer pipeline into recording manager(§15 コミット50相当)
  - `audio/pipeline.rs` 新設。`CapturePipeline` が cpal キャプチャ(mic/system/mix)とミキサースレッドを所有。
  - `RecordingIo`(WAVライター+セグメンター)を専用 Mutex に分離し、ミキサースレッドと stop 経路のデッドロックを回避。
  - stop はキャプチャ停止→ミキサー排出→セグメンターflush→WAV finalize→DB更新の順(§4.1)。flush 失敗時も finalize/DB更新を継続し、セッションを error にする。
  - DEVICE_LOST / WAV書き込み失敗時の自動停止(`capture_error_handler`)、MAX_RECORDING_MS 到達時の自動停止(`max_duration_handler`、§15 コミット55の実結線)を commands.rs に実装。
  - `TranscribeWorkerState` は `Arc` で manage する形に変更(sink スレッドへ渡すため)。
- `3c5b7a3` feat: implement real sound check capture with peak analysis(§15 コミット30の実キャプチャ化)
  - テストトーンのスタブを廃止し、`CapturePipeline`(ゲイン1.0)で実キャプチャ。ミキサーの100msレベルコールバックから `soundcheck://level` を発火。
  - ソース別サンプルピークを dBFS で報告(§5.7: <-60dB でほぼ無音警告、>-1dB で過大入力警告)。非対象ソースは null。
  - `run_sound_check` コマンドは `spawn_blocking` で非同期化。`SoundCheckResult.peakMicDb/peakSystemDb` は `number | null` に変更済み(types.ts / Record.tsx 対応済み)。
- `e078cdd` fix: read gpu mode from settings for each transcription job
  - `WhisperJobProcessor` はジョブ実行時に共有 `SettingsStore` から最新 `gpu_mode` を読む。設定変更後にワーカー再起動なしで次ジョブへ反映される。
- `fix: run import command work on blocking thread`
  - `import_files` コマンドを `async` 化し、デコード・WAV変換・DB投入・batchジョブ投入を `spawn_blocking` 側で実行する。進捗イベントと `ImportPipeline` の処理順は既存のまま。
- `feat: add model catalog and manifest module`
  - `models.rs` にモデルカタログ、`models/manifest.json` 読み書き、origin を含む manifest エントリ、§6.2.1 の usable/corrupted/manual 判定、`get_models` コマンドを追加。
  - app verified、app unverified、size mismatch corrupted、manifestなし手動配置、未DLの判定を単体テストで固定。
- `feat: add model download with streaming sha256 verification`
  - `download_model` コマンド、`model://progress` / `model://done` / `model://error` イベント、HF API metadata 取得、`.part` への streaming download、インクリメンタル SHA-256、完了時 manifest 更新を追加。
  - HF API metadata 取得失敗時はDLを継続し、実サイズ/実SHAで `verified=false, origin=app` として manifest に保存する。検証失敗・stream失敗時は `.part` を削除する。
- `feat: add cancel download and delete model commands`
  - `ModelDownloadManager` でモデルごとのアクティブDLとキャンセルフラグを管理し、重複DLを `MODEL_ALREADY_DOWNLOADING` で拒否。
  - `cancel_download` は進行中DLへキャンセルフラグを立て、stream write 経路で `CANCELED` として中断し `.part` を削除する。`delete_model` はモデル本体/`.part`/manifestエントリを削除し、更新後の `ModelInfo` を返す。
- `feat: add verify_model command`
  - `verify_model` コマンド、SHA-256再計算、HF API metadata 照合、manifest更新、手動配置モデルの usable 化経路を追加。
  - 検証失敗したモデルは manifest に `corrupted=true` として保持し、次回 `get_models` でも破損状態として返す。
- `feat: add model manager ui in settings`
  - `useModelStore` を追加し、`model://progress` / `model://done` / `model://error` を購読。
  - 設定画面にモデル一覧、状態バッジ、DL/キャンセル/削除/検証、DL進捗、usable モデル限定の既定モデル選択を追加。
  - 録音画面のモデル選択も backend の `get_models` 結果へ結線し、usable=false のモデルは選択・録音開始できないようにした。
- `feat: add onboarding flow with model download step`
  - `onboardingDone=false` 時の `/onboarding` 強制リダイレクトを追加。
  - ようこそ、モデル選択/DL、言語既定、完了の4ステップを実装。`medium-q5_0` を既定選択し、DL進捗/キャンセル/スキップに対応。

**注意: 録音・サウンドチェックの実機動作は未確認。** WSLからは Windows テストバイナリの実行までしか検証していない(122テストパス、clippy/fmt/pnpm build 通過)。実マイク/ループバックでの録音、DEVICE_LOST 自動停止、サウンドチェック再生は `docs/manual-windows-checks.md` に従い Windows 実機での確認が必要。

## 残タスク

### 1. モデル usable 判定のバックエンド開始経路結線

- `start_recording` / `import_files` / `retranscribe_session` 側の「usable モデルのみ許可」検証(§6.2.1 / §15 コミット26に記載)は未実装。現状は Record UI での抑止のみ。

### 2. フロントエンド残ギャップ(spec §8 総点検)

- §8 の画面仕様と現実装の突き合わせ。少なくとも以下を確認:
  - Record ページ: モデル選択が usable のみ活性になっているか(モデル管理実装後)。
  - `recording://limit` 警告(残10分)のUI表示。
  - サウンドチェック結果の再生ゲート(§9、`$APPDATA` scope)が実機で通るか。
- 完了後、§13 受入チェックリストを `docs/acceptance.md` に記録(コミット62)。

### 3. M6 残タスク(§15 コミット60〜62)

- コミット60: `pnpm tauri:build:win` による CPU版NSISクロスビルド成功確認。
- コミット61: README に build/release instructions、手動テスト手順、既知の制限、依存バージョン変更履歴を同期。
- コミット62: `docs/acceptance.md` に §13 の確認結果を記録し、必要ならタグ付け。

### 4. コード品質クリーンアップ / 実機リスク

- `/simplify` または `/code-review` を直近の変更(pipeline/recording/sound_check)にかける。
- 既知の設計メモ:
  - `cpal::Stream` は本来 `!Send` で、`unsafe impl Send`(capture.rs)により別スレッドからの drop を許容している(Windows 限定ターゲット前提)。実機で問題が出た場合はストリーム専有スレッド化を検討。
  - サウンドチェックの `SoundCheckManager.busy` ガードと `RecordingManager.recording_active` の間に TOCTOU の隙間がある(サウンドチェック中の録音開始は現状ブロックされない)。必要なら start_recording 側で sound check busy を確認する。
  - `npx tsc --noEmit` は node_modules の implicit @types(babel__core 等)で既存エラーが出る(実害なし)。気になる場合は tsconfig の `types` を明示する。

## ゲート(各コミット前、WSL2)

```
pnpm build
cd src-tauri && cargo fmt --all --check
cd src-tauri && cargo xwin check --target x86_64-pc-windows-msvc
cd src-tauri && cargo xwin clippy --target x86_64-pc-windows-msvc --all-targets -- -D warnings
cd src-tauri && cargo xwin test --target x86_64-pc-windows-msvc --no-run
```

テストバイナリは WSL の binfmt 経由で実行可能(今セッションで122件パスを確認)。実機挙動の証明にはならない点は `docs/manual-windows-checks.md` のポリシーに従う。
