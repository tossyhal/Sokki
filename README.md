# Sokki

Sokki は Windows 10/11 x64 専用の、ローカル完結型録音・文字起こしデスクトップアプリです。

v1では、マイク録音、システム音声録音、マイク+システム音声ミックス、ローカルWhisperによる文字起こし、音声ファイルインポート、txt/srt/mdエクスポート、音声テストを対象にします。

翻訳、話者分離、AI要約、全文検索、トランスクリプト編集はv1では実装しません。

詳細仕様は [`docs/spec.md`](docs/spec.md)、エージェント向け作業規約は [`AGENTS.md`](AGENTS.md) を参照してください。

## ドキュメント

- [`docs/spec.md`](docs/spec.md): v1 の正規仕様。仕様判断で迷った場合の正です。
- [`docs/acceptance.md`](docs/acceptance.md): v1 リリース候補の受け入れ状況。
- [`docs/manual-windows-checks.md`](docs/manual-windows-checks.md): WSL2 では代替できない Windows 実機確認手順。
- [`docs/design_token.md`](docs/design_token.md): UI トークンの抜粋。実装上の正は `docs/spec.md` §8.4 です。

## 開発環境

Sokki は Windows 専用アプリですが、開発環境は WSL2 を正規環境にできます。

WSL2上では Linux 向け Tauri アプリとしてビルドしません。
Windows向けには `cargo-xwin` を使って `x86_64-pc-windows-msvc` ターゲットへクロスビルドします。

WSL2からの `tauri dev` は正規の実行確認パスではありません。
WSL2ではWindows向け成果物をビルドし、生成されたWindows実行ファイルまたはNSISインストーラーをWindows上で実行確認します。

## 技術スタック

- Platform: Windows 10/11 x64
- App framework: Tauri v2
- Backend: Rust
- Frontend: React 18 + TypeScript + Vite + Tailwind CSS
- State: Zustand
- Database: SQLite (`rusqlite`)
- Audio: `cpal`, WASAPI loopback, `rubato`, `hound`
- 文字起こし: `whisper-rs` 経由の whisper.cpp
- Packaging: Tauri bundler / NSIS

## WSL2 Ubuntu のセットアップ

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

Linux向けTauriビルド用に `dbus`、GTK、WebKitGTK を入れる方針にはしません。

## 依存関係のインストール

```bash
pnpm install
```

このプロジェクトでは `npm` は使いません。

- `pnpm-lock.yaml` をコミットします
- `package-lock.json` はコミットしません

## チェック

```bash
pnpm build
pnpm typecheck

cd src-tauri
cargo fmt --all --check
cargo xwin check --target x86_64-pc-windows-msvc
cargo xwin clippy --target x86_64-pc-windows-msvc --all-targets -- -D warnings
cargo xwin test --target x86_64-pc-windows-msvc --all-targets --no-run
```

`cargo xwin clippy` が `cargo-xwin` 側の制約で動作しない場合のみ、その理由をこのREADMEに記録し、`cargo xwin check` を必須ゲート、clippy は Windows ネイティブまたは CI での補助ゲートとします。

## Windows向けNSISインストーラーのビルド

```bash
pnpm tauri:build:win
```

出力先:

```text
src-tauri/target/x86_64-pc-windows-msvc/release/bundle/nsis/
```

WSL2クロスビルドではNSISのみを対象にします。MSI/WiXビルドはWSL2クロスビルド対象外です。

配布候補は、CPU版NSISインストーラーを必須成果物とします。WSL2で `pnpm tauri:build:win` が通った後、`release/bundle/nsis/` 配下の `Sokki_*_x64-setup.exe` をWindows 10/11 x64上でインストール・起動確認してください。

リリース候補の確認結果は [`docs/acceptance.md`](docs/acceptance.md) に記録します。WSL2で代替できない項目は [`docs/manual-windows-checks.md`](docs/manual-windows-checks.md) の手順に従ってWindows実機で確認します。

## Windows上での実機確認

WSL2でビルドした後、以下はWindows上で確認してください。

- 生成された `.exe` が起動する
- NSIS installer からインストールできる
- WebView2 が動作する
- マイク録音できる
- WASAPI loopbackでシステム音声を録音できる
- マイク + システム音声ミックス録音ができる
- 録音中に使用中のマイクまたは出力デバイスを切断し、録音が自動停止してセッションが `error` 状態になり、保存済みWAVが再生できる
- 録音WAVとサウンドチェックWAVをWebViewから再生できる
- assetProtocol scope が正しく機能する

詳細な手動確認手順は [`docs/manual-windows-checks.md`](docs/manual-windows-checks.md) にまとめています。エクスポート、モデル管理、オンボーディング、インポート、リアルタイム文字起こし、復旧系の確認も同ファイルに従ってください。

## GPUビルド

MVPの必須受け入れ対象はCPU版NSIS installerです。

`gpu-vulkan` feature付きのクロスビルドは任意検証とし、失敗してもMVPの受け入れをブロックしません。
GPU版リリースが必要な場合は、Windowsネイティブビルドを正とします。

任意検証用のコマンドは以下です。

```bash
pnpm tauri:build:win:gpu
```

## プライバシー

- 録音音声はローカルWAVとして保存します。
- 文字起こし結果はローカルSQLiteに保存します。
- モデルダウンロードとモデル検証以外で音声・文字起こし本文を外部送信しません。
- フォントはローカルバンドルします。Google Fonts等のCDN参照は使いません。

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

## 注意事項

- WSL2上で plain `cargo tauri build` を実行しないでください
- Linux向けTauriビルド用に `dbus`、GTK、WebKitGTK を入れないでください
- WSL2クロスビルドではNSISのみを対象にします
- MSI/WiXビルドはWSL2クロスビルド対象外です
- GPU/VulkanクロスビルドはMVP必須ではありません

## 既知の制限

- 対象OSはWindows 10/11 x64のみです。
- v1では翻訳・話者分離・要約・検索・編集はありません。
- デバイスIDはcpalの制約によりデバイス名ベースです。再接続・同名デバイスでは注意が必要です。
- マイクとシステム音声の厳密同期はv1では行いません。
- CPU環境ではリアルタイム文字起こしが遅れる場合があります。その場合もキュー滞留をbusy表示し、録音自体は継続します。
- バッチ文字起こしは最大15秒チャンクです。録音開始時はリアルタイム文字起こしを優先しますが、環境によっては実行中チャンク1回分の推論完了待ちが発生する可能性があります。

## 依存バージョン変更履歴

依存バージョンの正は [`docs/spec.md`](docs/spec.md) §1.3 です。変更が必要な場合は、`Cargo.toml` / `Cargo.lock` または `package.json` / `pnpm-lock.yaml` と同じコミットで理由をここに追記します。

- 2026-07-09: Tauri v2 / React 18 / whisper-rs 0.16 系を `docs/spec.md` §1.3 に合わせて使用。追加のバージョン逸脱はありません。

## ライセンス

未定。
