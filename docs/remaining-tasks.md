# 残タスク一覧(2026-07-09 時点)

作業を中断するにあたり、現状と残タスクをまとめる。実装単位の正は `docs/spec.md` §15 のコミット計画。

## 完了済み(今回のセッション)

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

**注意: 録音・サウンドチェックの実機動作は未確認。** WSLからは Windows テストバイナリの実行までしか検証していない(122テストパス、clippy/fmt/pnpm build 通過)。実マイク/ループバックでの録音、DEVICE_LOST 自動停止、サウンドチェック再生は `docs/manual-windows-checks.md` に従い Windows 実機での確認が必要。

## 残タスク

### 1. import の非同期化

- **gpu_mode 陳腐化は修正済み**: `WhisperJobProcessor` はジョブ実行時に共有 `SettingsStore` から最新 `gpu_mode` を読む。
- **import_files が同期コマンド**: `commands.rs` の `import_files` は同期実行のため、長時間ファイルのデコード中はメインスレッドがブロックされる。`run_sound_check` と同様に `spawn_blocking` 化する(進捗イベントは既存のまま)。

### 2. モデル管理バックエンド(§15 コミット12〜15)— 未着手

- コミット12: モデルカタログ定数、`manifest.json` 読み書き(origin 含む)、§6.2.1 の usable 判定ロジック、`get_models` コマンド。判定の単体テスト(app/manual/破損/未検証の全パターン)。
- コミット13: HF API 取得→manifest キャッシュ、streaming ダウンロード+インクリメンタル sha256、`.part`→リネーム、progress/done/error イベント、API 失敗時の verified=false 経路。
- コミット14: `cancel_download` / `delete_model` コマンド。
- コミット15: `verify_model` コマンド(SHA再計算+HF照合、手動配置モデルの usable 化)。
- 現状 `src-tauri/src` に catalog/manifest 関連のモジュールは存在しない。`start_recording` 側の「usable モデルのみ許可」検証(§15 コミット26に記載)も未実装のはず — 実装時に結線すること。

### 3. モデル管理フロントエンド+オンボーディング(§15 コミット16〜17)— 未着手

- コミット16: 設定画面のモデルマネージャUI(一覧、状態バッジ: 未DL/DL済/未検証/手動配置・未検証/破損、DL進捗、各操作ボタン、デフォルトモデル選択)、`useModelStore`。
- コミット17: オンボーディング4ステップ(medium-q5_0 既定、スキップ・再試行、未完了時の強制リダイレクト)。

### 4. フロントエンド残ギャップ(spec §8 総点検)

- §8 の画面仕様と現実装の突き合わせ。少なくとも以下を確認:
  - Record ページ: モデル選択が usable のみ活性になっているか(モデル管理実装後)。
  - `recording://limit` 警告(残10分)のUI表示。
  - サウンドチェック結果の再生ゲート(§9、`$APPDATA` scope)が実機で通るか。
- 完了後、§13 受入チェックリストを `docs/acceptance.md` に記録(コミット62)。

### 5. コード品質クリーンアップ

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
