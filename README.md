# Sokki

Sokkiは、Windows上で授業・会議・動画音声を録音し、ローカルで文字起こしするデスクトップアプリです。

音声と文字起こしデータはPC内に保存されます。外部通信は、Whisperモデルのダウンロードとモデル検証に必要な通信に限定します。

## v1の方針

Sokki v1は、**ローカル完結型の録音・文字起こしアプリ**です。

v1では以下を実装します。

- マイク録音
- システム音声録音（WASAPIループバック）
- マイク + システム音声のミックス録音
- リアルタイム文字起こし
- 録音後の一括文字起こし
- 音声ファイルのインポート文字起こし
- txt / srt / md エクスポート
- 音声テスト / サウンドチェック
- Whisperモデルのダウンロード・検証・管理
- GPUモード切替（auto / CPU固定 / GPU固定）
- NSISインストーラー配布

v1では以下を実装しません。

- 翻訳
- 話者分離
- AI要約
- 全文検索
- トランスクリプト編集
- 静音モード / パフォーマンスプリセット
- 自動アップデータ

詳細は [`spec.md`](./spec.md) を参照してください。

## 技術スタック

- Platform: Windows 10/11 x64
- App framework: Tauri v2
- Backend: Rust
- Frontend: React 18 + TypeScript + Vite + Tailwind CSS
- State: Zustand
- Database: SQLite (`rusqlite`)
- Audio: `cpal`, WASAPI loopback, `rubato`, `hound`
- Transcription: whisper.cpp via `whisper-rs`
- Packaging: Tauri bundler / NSIS

## プライバシー

- 録音音声はローカルWAVとして保存します。
- 文字起こし結果はローカルSQLiteに保存します。
- モデルダウンロード以外で音声・文字起こし本文を外部送信しません。
- フォントはローカルバンドルします。Google Fonts等のCDN参照は使いません。

## ディレクトリ構成

```text
sokki/
├─ src/                       # React frontend
│  ├─ pages/
│  ├─ components/
│  ├─ stores/
│  ├─ lib/
│  └─ styles/
├─ src-tauri/                 # Rust backend / Tauri
│  ├─ src/
│  │  ├─ audio/
│  │  ├─ whisper/
│  │  ├─ models.rs
│  │  ├─ import.rs
│  │  ├─ export.rs
│  │  └─ db.rs
│  ├─ capabilities/
│  └─ Cargo.toml
├─ AGENT.md                   # agent implementation rules
├─ spec.md                    # authoritative implementation spec
└─ README.md
```

## ローカルデータ

WindowsではTauriの `app_data_dir()` 配下に保存します。

```text
%APPDATA%\com.sokki.app\
├─ sokki.db
├─ settings.json
├─ recordings\{session_id}.wav
├─ soundcheck\test.wav
└─ models\
   ├─ ggml-*.bin
   └─ manifest.json
```

## セットアップ

```bash
npm install
npm run tauri dev
```

Rust側のみ確認する場合:

```bash
cd src-tauri
cargo check
cargo fmt --all
cargo clippy --all-targets -- -D warnings
```

フロントエンドのビルド確認:

```bash
npm run build
```

## ビルド

CPU版ビルド:

```bash
npm run tauri build
```

GPU/Vulkan対応ビルド:

```bash
npm run tauri build -- --features gpu-vulkan
```

GPUビルドにはVulkan SDKが必要です。詳細な導入手順と配布手順は実装完了後にこのREADMEへ追記します。

## 開発時の必須チェック

各コミット前に以下を通します。

```bash
npm run build
cd src-tauri
cargo fmt --all
cargo clippy --all-targets -- -D warnings
cargo check
```

UIを含む変更では `npm run tauri dev` で目視確認します。

## 実装ドキュメント

- [`AGENT.md`](./AGENT.md): 実装エージェント向けの作業規約、禁止事項、ゲート条件
- [`spec.md`](./spec.md): 詳細な実装仕様、データモデル、API、音声処理、Whisper統合、受け入れ基準、コミット計画
- `docs/acceptance.md`: v1.0.0リリース前の受け入れ結果記録用。実装後に作成します。

## 既知の制限

- 対象OSはWindows 10/11 x64のみです。
- v1では翻訳・話者分離・要約・検索・編集はありません。
- デバイスIDはcpalの制約によりデバイス名ベースです。再接続・同名デバイスでは注意が必要です。
- マイクとシステム音声の厳密同期はv1では行いません。
- CPU環境ではリアルタイム文字起こしが遅れる場合があります。その場合もキュー滞留をbusy表示し、録音自体は継続します。

## ライセンス

未定。
