# AGENTS.md

このファイルは、Sokki を実装するエージェント向けの作業規約です。詳細仕様の正は [`docs/spec.md`](docs/spec.md) です。`README.md` は人間向けの概要・セットアップ・利用説明です。

## プロジェクト方針

Sokki は Windows 10/11 x64 専用の Tauri v2 デスクトップアプリです。
WSL2 を正規の開発環境としますが、Tauri を Linux デスクトップアプリとしてビルドしてはいけません。

WSL2上で plain `cargo tauri build` を実行して Linux ターゲットをビルドすることは禁止します。
`dbus`、GTK、WebKitGTK などの Linux Tauri デスクトップ依存を、このプロジェクトのために導入しないでください。

正規のWindowsターゲットは以下です。

- `x86_64-pc-windows-msvc`
- NSIS installer のみ
- `cargo-xwin` によるクロスビルド

WSL2からの `tauri dev` は正規の実行確認パスではありません。
WSL2ではWindows向け成果物をビルドし、生成されたWindows実行ファイルまたはNSISインストーラーをWindows上で実行確認します。

## 最優先ルール

1. **`docs/spec.md` を正とする。** 迷ったら `docs/spec.md` の決定に従う。
2. **v1では翻訳・話者分離・AI要約・全文検索・編集は実装しない。** 将来拡張を阻害しない構造に留める。
3. **完全ローカル方針を守る。** 外部通信はモデルダウンロードとモデル検証用の Hugging Face API のみ。
4. **Windows 10/11 x64 のみを対象にする。** 他OS対応の分岐や抽象化を勝手に増やさない。
5. **Tauri v2 APIのみ使用する。** v1 の `tauri::api::*` や allowlist 記法は禁止。
6. **フロントエンドから直接ファイルシステムへ触らない。** ファイル操作はRustコマンド経由に限定する。
7. **依存バージョンは `docs/spec.md` §1.3 を正とする。** 勝手に最新版へ上げない。変更が必要な場合はREADMEに理由を残し、`Cargo.toml` / `Cargo.lock` を同時更新する。
8. **`Cargo.lock` と `pnpm-lock.yaml` は必ずコミットする。**
9. **コミット順・粒度・メッセージは `docs/spec.md` §15 に従う。** 1項目=1コミットを原則にする。
10. **各コミット前にビルド・lint・formatを通す。** §15 の運用規則に従う。

## WSL2セットアップ

WSL2 Ubuntuでは以下を用意します。

```bash
sudo apt update
sudo apt install -y \
  build-essential curl wget file pkg-config libssl-dev \
  clang-19 lld-19 llvm-19 cmake ninja-build nsis

rustup target add x86_64-pc-windows-msvc
cargo install --locked cargo-xwin
corepack enable
pnpm --version
```

`whisper-rs` がビルドする `whisper.cpp` は、cargo-xwin の MSVC STL との組み合わせで Clang 19 以上を必要とします。

## パッケージマネージャ

このプロジェクトでは `pnpm` のみを使います。

- `npm` は使わない
- `package-lock.json` はコミットしない
- `pnpm-lock.yaml` は必ずコミットする
- フロントエンド依存を変更した場合は `pnpm-lock.yaml` の変更も同じコミットに含める

## 各コミット前の必須チェック

各コミット前にWSL2上で以下を実行します。

```bash
pnpm build

cd src-tauri
cargo fmt --check
cargo xwin check --target x86_64-pc-windows-msvc
cargo xwin clippy --target x86_64-pc-windows-msvc --all-targets -- -D warnings
```

`cargo xwin clippy` が `cargo-xwin` 側の制約で動作しない場合のみ、その理由を `README.md` に記録し、`cargo xwin check` を必須ゲート、clippy は Windows ネイティブまたは CI での補助ゲートとします。

## Windows向けビルド

正規のWindows向けNSISビルドは以下です。

```bash
pnpm tauri:build:win
```

出力先:

```text
src-tauri/target/x86_64-pc-windows-msvc/release/bundle/nsis/
```

## 実機確認

WSL2でのクロスビルド完了後、以下はWindows上で確認します。

- 生成された `.exe` が起動する
- NSIS installer からインストールできる
- WebView2 が動作する
- マイク録音できる
- WASAPI loopback でシステム音声を録音できる
- マイク + システム音声ミックス録音ができる
- 録音WAVとサウンドチェックWAVをWebViewから再生できる
- assetProtocol scope が正しく機能する

## 実装対象の境界

### v1で実装する

- Windows向けTauri v2デスクトップアプリ
- React 18 + TypeScript + Vite + Tailwind CSS
- SQLite + WAVローカル保存
- whisper.cpp / whisper-rs による完全ローカル文字起こし
- マイク録音
- WASAPIループバックによるシステム音声録音
- マイク + システム音声ミックス録音
- リアルタイム文字起こし
- 録音後一括文字起こし
- 音声ファイルインポート文字起こし
- txt / srt / md エクスポート
- モデルDL・SHA-256検証・未検証/破損モデル管理
- GPUモード `auto / force_cpu / force_gpu`
- 音声テスト / サウンドチェック
- NSISインストーラー

### v1で実装しない

- 翻訳
- 話者分離
- AI要約
- 全文検索
- トランスクリプト編集
- モデルDLレジューム
- 自動アップデータ
- 永続ジョブキュー
- mic-system厳密時刻同期

## 絶対に破ってはいけない技術制約

### audio callback

`cpal` の audio callback 内では以下を禁止する。

- `Vec` の新規確保
- `Mutex` ロック
- ログ出力
- Tauri event emit
- ブロッキング操作
- 重い変換処理

callbackでは、事前確保済みの固定長バッファへコピーし、lock-freeチャネルへ `try_send` するだけにする。ドロップ時は `AtomicU64` の `drop_count` を増やすだけにし、ログやUI通知はMixer側で行う。

### バッチ文字起こし

- 音声全体を1ジョブとしてWhisperへ渡してはならない。
- バッチチャンクは**最大15秒**。
- batchジョブには abort callback を設定し、`recording_active == true` になったら中断する。
- 中断されたbatchジョブは結果を破棄し、`deferred` に退避する。
- `deferred` 再処理直前にも `canceled` を確認する。
- `import://progress` は正常完了したチャンクの valid 音声時間だけを加算する。

### リアルタイム文字起こし

- リアルタイムSegmenterは最大5秒で強制確定する。
- 5秒強制確定時は次チャンク先頭に600msオーバーラップを付ける。
- オーバーラップとプリロールは認識安定化のためだけに使い、DB保存は `valid_start_ms / valid_end_ms` の範囲に限定する。
- 同一発話の二重セグメントを避けるため、`docs/spec.md` §6.3.1 の重複抑制を実装する。

### stop_recording

`stop_recording` はWhisper完了を待ってはならない。

1. Mixer / capture停止
2. Segmenter flush
3. WAV finalize
4. `duration_ms` / `drop_count` をDB確定
5. `pending_job_count > 0` なら `transcribing`、0なら `done`
6. 即座に `Session` を返す
7. 以後の `done` 遷移は `JobTracker` のみが行う

### JobTracker

- ジョブ投入時に `pending[session] += 1`。
- 成功・破棄・エラー・キャンセルスキップのいずれでも必ず `-1`。
- deferred退避時は未完了なので減らさない。
- `pending == 0` かつ録音中でない場合のみ `done` にする。
- `error` 状態は `done` で上書きしない。
- decrement と status判定は同一クリティカルセクションで行う。

### 起動時復旧

migration直後に以下を実行する。

- `status='recording'` は `interrupted` に変更し、WAVヘッダ修復を試みる。
- `status='transcribing'` は `interrupted` に変更する。メモリ上のジョブキューは復元しない。
- 詳細画面では再文字起こし導線を表示する。

### asset protocol

- 音声ファイルパスはRust側で `app_data_dir()` から絶対パスを生成する。
- フロントでパスを組み立てない。
- フロントは常に `convertFileSrc(audioPath)` / `convertFileSrc(wavPath)` を使う。
- M3でサウンドチェックWAVと録音WAVの実再生を確認する。
- 再生ゲートを通過するまで次の実装へ進まない。

## デザイン実装ルール

- デザインの正は `docs/spec.md` §8.4 のトークン。
- Claude Design由来の過去モックはコード流用禁止。実装時の正は `docs/spec.md` §8.4 のトークンとする。
- Google Fonts等のCDN参照は禁止。Inter / Noto Sans JP はローカルバンドルする。
- アクセント色 `#C4453F` はRECドット、主アクション、文字起こし中バッジ、再生位置ハイライトに限定する。
- モーションは `rec-pulse` と `seg-in` の2つのみ。
- `<form>` のネイティブsubmitは禁止。`onClick` / `onChange` で制御する。

## 実装順

正確なコミット計画は `docs/spec.md` §15 に従う。大枠は以下。

1. M1: スキャフォールド
2. M2: DB・設定・モデル管理・Onboarding
3. M3: 録音 + 音声テスト
4. M4: ジョブ基盤 + バッチ文字起こし + エクスポート
5. M5: リアルタイム文字起こし
6. M6: 仕上げ・配布

## チェックリスト

各段階で `docs/spec.md` §13 の受け入れ基準を満たすこと。特に以下はゲート扱いにする。

- CPU版NSISクロスビルドがVulkan SDKなしで成功する
- モデルDL後にSHA-256検証される
- マイク / システム音声 / ミックスの3構成で録音できる
- サウンドチェックWAVと録音WAVがWebViewで再生できる
- バッチ処理中に録音開始したらrtが優先される
- オーバーラップ部で二重セグメントが出ない
- `stop_recording` が即返る
- `transcribing` 中に強制終了しても次回起動で `interrupted` になる
- srtがVLC等で読み込める
- NSISビルドが成功する

## 禁止事項

- WSL2上でLinux向けTauriビルドを正規ゲートにしない
- `dbus` / GTK / WebKitGTK 依存をこのプロジェクトのために入れない
- `npm install` を使わない
- `package-lock.json` を作らない
- `tauri dev` from WSL2 を正規の実行確認にしない
- v1仕様にない機能を勝手に追加しない

## GPUビルド

MVPの必須受け入れ対象はCPU版NSIS installerです。

`gpu-vulkan` feature付きのクロスビルドは任意検証とし、失敗してもMVPの受け入れをブロックしません。
GPU版リリースが必要な場合は、Windowsネイティブビルドを正とします。
