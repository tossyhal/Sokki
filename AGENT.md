# AGENT.md

このファイルは、Sokki を実装するエージェント向けの作業規約です。詳細仕様の正は [`spec.md`](./spec.md) です。`README.md` は人間向けの概要・セットアップ・利用説明です。

## 最優先ルール

1. **`spec.md` を正とする。** 迷ったら `spec.md` の決定に従う。
2. **v1では翻訳・話者分離・AI要約・全文検索・編集・静音モードは実装しない。** 将来拡張を阻害しない構造に留める。
3. **完全ローカル方針を守る。** 外部通信はモデルダウンロードとモデル検証用の Hugging Face API のみ。
4. **Windows 10/11 x64 のみを対象にする。** 他OS対応の分岐や抽象化を勝手に増やさない。
5. **Tauri v2 APIのみ使用する。** v1 の `tauri::api::*` や allowlist 記法は禁止。
6. **フロントエンドから直接ファイルシステムへ触らない。** ファイル操作はRustコマンド経由に限定する。
7. **依存バージョンは `spec.md` §1.3 を正とする。** 勝手に最新版へ上げない。変更が必要な場合はREADMEに理由を残し、`Cargo.toml` / `Cargo.lock` を同時更新する。
8. **`Cargo.lock` と `package-lock.json` は必ずコミットする。**
9. **コミット順・粒度・メッセージは `spec.md` §15 に従う。** 1項目=1コミットを原則にする。
10. **各コミット前にビルド・lint・formatを通す。** 少なくとも以下を実行する。

```bash
npm run build
cd src-tauri
cargo fmt --all
cargo clippy --all-targets -- -D warnings
cargo check
```

UIを含むコミットでは `npm run tauri dev` で目視確認も行う。

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
- 静音モード / パフォーマンスプリセット

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

- リアルタイムSegmenterは最大8秒で強制確定する。
- 8秒強制確定時は次チャンク先頭に600msオーバーラップを付ける。
- オーバーラップとプリロールは認識安定化のためだけに使い、DB保存は `valid_start_ms / valid_end_ms` の範囲に限定する。
- 同一発話の二重セグメントを避けるため、`spec.md` §6.3.1 の重複抑制を実装する。

### stop_recording

`stop_recording` はWhisper完了を待ってはならない。

手順は以下。

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

- デザインの正は `spec.md` §8.4 のトークン。
- Claude Design由来の `Sokki.html` は視覚参照のみ。コード流用は禁止。
- Google Fonts等のCDN参照は禁止。Inter / Noto Sans JP はローカルバンドルする。
- アクセント色 `#C4453F` はRECドット、主アクション、文字起こし中バッジ、再生位置ハイライトに限定する。
- モーションは `rec-pulse` と `seg-in` の2つのみ。
- `<form>` のネイティブsubmitは禁止。`onClick` / `onChange` で制御する。

## 実装順

正確なコミット計画は `spec.md` §15 に従う。大枠は以下。

1. M1: スキャフォールド
2. M2: DB・設定・モデル管理・Onboarding
3. M3: 録音 + 音声テスト
4. M4: ジョブ基盤 + バッチ文字起こし + エクスポート
5. M5: リアルタイム文字起こし
6. M6: 仕上げ・配布

## チェックリスト

各段階で `spec.md` §13 の受け入れ基準を満たすこと。特に以下はゲート扱いにする。

- CPUのみビルドがVulkan SDKなしで成功する
- モデルDL後にSHA-256検証される
- マイク / システム音声 / ミックスの3構成で録音できる
- サウンドチェックWAVと録音WAVがWebViewで再生できる
- バッチ処理中に録音開始したらrtが優先される
- オーバーラップ部で二重セグメントが出ない
- `stop_recording` が即返る
- `transcribing` 中に強制終了しても次回起動で `interrupted` になる
- srtがVLC等で読み込める
- NSISビルドが成功する
