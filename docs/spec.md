<!--
This is the authoritative implementation specification for Sokki v1.
AGENT.md contains implementation rules for autonomous agents.
README.md contains the human-facing project overview and setup notes.
-->

# spec.md - Sokki 実装仕様 v1.3

**ローカル完結型の録音・文字起こしデスクトップアプリ。**
本書は Cursor Composer 2.5 が自律的に実装を完遂できることを目的とした実装仕様書である。曖昧な点は本書の決定に従い、本書に記載のない些末な判断は一般的なベストプラクティスに従うこと。

> **v1.2 からの主な変更(レビュー第3弾+デザイン確定反映)**
> 1. リアルタイムSegmenterの最大チャンク長を **18秒→8秒** に短縮(ライブ表示の初回遅延対策)(§5.5)
> 2. assetProtocol scope を `**/*` パターンに変更(§9)
> 3. モデルDLの記述を「ユーザーが選択したモデルのみDL」に明確化(§0)
> 4. deferred ジョブ再処理直前の canceled 確認を明記(§1.2)
> 5. `import://progress` は正常完了チャンクの valid 時間のみ加算と明記(§6.4)
> 6. schema_meta を key-value 形式に変更(§3.1)
> 7. **§8.4 デザイントークン(確定値)/ §8.5 デザインモック参照と差分** を新設。コミット2を確定トークン適用に変更

> **v1.1 からの主な変更(レビュー第2弾反映)**
> 1. バッチチャンクを最大60秒→**最大15秒**に短縮し、**abort callback による preempt を必須化**(§1.2, §6.4)
> 2. **オーバーラップ範囲の重複除去**を定義: TranscribeJob に valid_start_ms / valid_end_ms を追加(§6.3.1)
> 3. **起動時の transcribing 復旧**を定義(§3.4)
> 4. `import_files` の戻り値を **per-file の ImportFileResult[]** に変更(§4)
> 5. **未検証モデルの使用可否ポリシー**を定義(§6.2.1)
> 6. assetProtocol / wavPath の扱いを厳密化、M3 に再生ゲートを設置(§9)
> 7. **依存バージョン固定方針を強化**(「最新安定版に読み替えてよい」を削除、Cargo.lock コミット必須)(§1.3)
> 8. ループバック実装を **LoopbackCapture trait で隔離**、wasapi crate 切替を条件付き別コミット化(§5.3, §15)
> 9. **pending_job_count による完了判定**を定義し、done 更新の race を排除(§6.3.2)
> 10. コミット計画を改訂: ジョブ管理土台を worker より先に、重複除去を batch 段階(M4)で実装

---

## 0. 確定済み要件サマリ

| 項目 | 決定内容 |
|---|---|
| プロダクト説明(v1) | **「ローカル完結型の録音・文字起こしデスクトップアプリ」**。"Notta代替" とは名乗らない。翻訳は v1 では実装しない |
| プラットフォーム | **Windows 10/11 (x64) のみ** |
| フレームワーク | **Tauri v2** + Rust バックエンド |
| フロントエンド | **React 18 + TypeScript + Vite + Tailwind CSS + Zustand + React Router** |
| 文字起こし | **whisper.cpp(whisper-rs)、完全ローカル**。外部通信はモデルDLのみ |
| 文字起こしモード | **リアルタイム(録音中逐次表示)と録音後一括の両対応** |
| 録音ソース | **マイク / システム音声(WASAPIループバック)/ 両方ミックス** を選択可能 |
| GPU | ビルド: cargo feature で GPU(Vulkan)分離、CPU単体ビルド必須成立。実行: `gpu_mode`(auto / force_cpu / force_gpu) |
| 対象言語 | **日本語 / 英語 / 自動判別** |
| モデル | アプリに同梱せず、**ユーザーが選択したモデルのみ**初回起動時等にDL(SHA-256検証)。Onboarding既定選択は **medium-q5_0**(バランス推奨) |
| 付加機能(v1) | 音声ファイルのインポート文字起こし、エクスポート(txt / srt / md)、音声テスト |
| Phase 2 | 翻訳、話者分離、AI要約、全文検索、トランスクリプト編集、モデルDLレジューム、自動アップデータ、永続ジョブキュー |
| 配布 | **NSISインストーラー**(Tauri bundler) |
| データ保存 | **SQLite**(rusqlite)+ WAV、すべてローカル |

---

## 1. アーキテクチャ概要

```
┌─────────────────────────── Tauri App ───────────────────────────┐
│  ┌── WebView (React) ──┐        ┌──────── Rust Core ─────────┐  │
│  │ Pages / Zustand     │ invoke │ commands.rs                 │  │
│  │ stores              │ ─────► │        │                    │  │
│  │                     │ events │        ▼                    │  │
│  │ transcript UI       │ ◄───── │ AppState (Mutex)            │  │
│  └─────────────────────┘        │  ├ RecordingManager         │  │
│                                 │  ├ JobTracker               │  │
│                                 │  ├ TranscribeWorker(常駐)   │  │
│                                 │  ├ ModelManager             │  │
│                                 │  ├ ImportManager            │  │
│                                 │  └ Db (rusqlite)            │  │
│                                 └────────────────────────────┘  │
└──────────────────────────────────────────────────────────────────┘

音声データフロー(録音時):
cpal入力(mic) ──┐  バッファプール経由
                 ├→ [resample→16kHz mono f32] → Mixer ─┬→ WAV書き込み(hound)
cpal loopback ──┘                                      ├→ Segmenter(VAD) → rtジョブ → TranscribeWorker
                                                        └→ レベル計算 → recording://level

バッチ(インポート/再文字起こし):
音声ファイル → symphoniaデコード → 16kHz mono → WAV保存
            → オフライン分割(≤15sチャンク+valid範囲) → batchジョブ列 → TranscribeWorker
```

### 1.1 スレッドモデル
- **cpal audio callback スレッド**(mic / loopback、cpal管理): §5.2 のバッファプール方式でサンプルを渡すのみ。**callback内でのアロケーション・Mutexロック・ログ出力・emit・ブロッキング操作は禁止**。
- **Mixerスレッド**(録音中のみ): 2本のチャネルから受信し、リサンプル→ミックス→ WAV / Segmenter / レベル計算へ供給。
- **TranscribeWorkerスレッド**(常駐1本): §1.2 のスケジューリング規則に従い rt / batch ジョブを処理。WhisperContext はモデル切替まで保持。
- **ダウンロードタスク**: tauri の async runtime 上で spawn。

### 1.2 ジョブスケジューリング規則(重要)

```rust
// AppState 内: recording_active: Arc<AtomicBool>  (= preempt フラグを兼ねる)
loop {
    if let Ok(job) = rt_rx.try_recv() { process(job); continue; }      // rt最優先
    if recording_active.load() { park_timeout(50ms); continue; }        // 録音中はbatch完全停止
    if let Some(job) = deferred.take() {                                 // 中断されたチャンクを最優先で再開
        // 再処理直前にも canceled を確認。true なら処理せず pending を -1 する
        if !job.canceled() { process(job); } else { tracker.finish(&job.session_id); }
        continue;
    }
    select_timeout(50ms) {
        recv(rt_rx)    -> job => process(job),
        recv(batch_rx) -> job => { if !job.canceled() { process(job) } },
    }
}
```

rt遅延を確実に抑えるため、以下を**すべて**実装する:
1. **バッチジョブは必ず15秒以下のチャンク単位**で投入(§6.4)。音声全体や60秒級を1ジョブとして whisper に渡すことを**禁止**。
2. **abort callback を必須実装**: batch ジョブの推論時、whisper の abort callback(whisper-rs の `set_abort_callback_safe` 等、使用バージョンで利用可能なAPI)に「`recording_active` が true なら中断」を設定する。中断されたジョブは結果を破棄し、`deferred` スロットに退避して録音終了後に**先頭から再処理**する(pending カウントは減らさない)。
3. 万一、使用バージョンの whisper-rs で abort callback が利用できないことが実装時に判明した場合でも、1. の15秒上限により「録音開始→最初のrt結果」のブロックは最大15秒チャンク1回分の推論時間に抑えられる(この場合は README の既知の制限に明記する)。
- キャンセルはチャンク境界で判定(sessionId ごとの `Arc<AtomicBool>`)。
- 録音停止後の flush で生成された rt ジョブも rt キュー扱い。batch は「録音非アクティブ かつ rt キュー空」のときのみ進む。

### 1.3 crate 依存とバージョン固定方針(厳守)

```toml
[dependencies]
tauri = { version = "2", features = [] }
tauri-plugin-dialog = "2"
tauri-plugin-opener = "2"
tauri-plugin-single-instance = "2"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
rusqlite = { version = "0.31", features = ["bundled"] }
cpal = "0.15"
hound = "3.5"
rubato = "0.15"
crossbeam-channel = "0.5"
whisper-rs = { version = "0.16" }
symphonia = { version = "0.5", features = ["mp3", "aac", "isomp4", "flac", "wav", "ogg", "vorbis"] }
reqwest = { version = "0.12", features = ["stream", "json"] }
sha2 = "0.10"
tokio = { version = "1", features = ["fs", "io-util"] }
uuid = { version = "1", features = ["v4"] }
chrono = "0.4"
thiserror = "1"
log = "0.4"
env_logger = "0.11"
sysinfo = "0.30"
```

- **本仕様のバージョン指定を正とする。** 実装中にAPI不整合が出た場合、勝手に最新版へ上げず、まず**指定バージョンのdocsに合わせて実装**する。
- どうしても変更が必要な場合のみ、README にバージョンと理由を記録して Cargo.toml / Cargo.lock を更新する。
- **Cargo.lock / package-lock.json は必ずコミットする。**

### 1.4 GPU方針
```toml
[features]
default = []
gpu-vulkan = ["whisper-rs/vulkan"]
```
- **開発・CIビルドはデフォルト(CPUのみ)で必ず通ること**。リリースは `cargo tauri build --features gpu-vulkan`(要 Vulkan SDK。READMEに導入手順)。
- 実行時は `gpu_mode` に従う:
  - `auto`: `use_gpu(true)` でロード → 失敗したら `use_gpu(false)` で再試行し、失敗理由を `gpuErrorMessage` に保持
  - `force_cpu`: 常に `use_gpu(false)`
  - `force_gpu`: `use_gpu(true)` のみ。失敗時 `WHISPER_GPU_UNAVAILABLE`
- CPUのみビルド(`compiledGpuSupport=false`)では常にCPU動作とし、設定UIで gpu 系選択肢を無効表示。

---

## 2. ディレクトリ構成

```
sokki/
├─ src/                       # React
│  ├─ main.tsx
│  ├─ App.tsx                 # Router定義・initEventListeners呼び出し
│  ├─ pages/
│  │  ├─ Onboarding.tsx / Library.tsx / Record.tsx / SessionDetail.tsx / Settings.tsx
│  ├─ components/
│  │  ├─ LevelMeter.tsx / SoundCheck.tsx / TranscriptView.tsx / SegmentRow.tsx
│  │  ├─ AudioPlayer.tsx / ModelManager.tsx / SessionCard.tsx
│  │  ├─ ExportDialog.tsx / ConfirmDialog.tsx / Toast.tsx
│  ├─ stores/
│  │  ├─ useSessionStore.ts / useRecordingStore.ts / useModelStore.ts / useSettingsStore.ts
│  ├─ lib/
│  │  ├─ api.ts / events.ts / types.ts / format.ts
│  └─ styles/index.css
├─ src-tauri/
│  ├─ src/
│  │  ├─ main.rs
│  │  ├─ lib.rs               # AppState組み立て、plugin登録、起動時復旧処理(§3.4)
│  │  ├─ commands.rs / state.rs / db.rs
│  │  ├─ audio/
│  │  │  ├─ mod.rs / devices.rs / buffer_pool.rs
│  │  │  ├─ capture.rs        # LoopbackCapture trait + CpalLoopbackCapture(§5.3)
│  │  │  ├─ resample.rs / mixer.rs / wav.rs / segmenter.rs / sound_check.rs
│  │  ├─ whisper/
│  │  │  ├─ mod.rs            # コンテキスト管理・パラメータ・後処理(重複除去含む)
│  │  │  ├─ jobs.rs           # TranscribeJob定義・JobTracker(§6.3.2)
│  │  │  └─ worker.rs         # 常駐ワーカー・スケジューラ・abort callback
│  │  ├─ models.rs / import.rs / export.rs / error.rs
│  ├─ capabilities/default.json
│  ├─ tauri.conf.json
│  └─ Cargo.toml
├─ package.json
└─ README.md
```

### アプリデータ配置(実行時)
Tauri v2 の `app_data_dir()`(Windowsでは `%APPDATA%\com.sokki.app\` に解決)
```
├─ sokki.db
├─ settings.json
├─ recordings/{session_id}.wav
├─ soundcheck/test.wav
└─ models/
   ├─ ggml-*.bin
   └─ manifest.json
```

---

## 3. データモデル

### 3.1 SQLite スキーマ(db.rs、起動時 migration)
```sql
PRAGMA journal_mode=WAL;
PRAGMA foreign_keys=ON;

CREATE TABLE IF NOT EXISTS sessions (
  id          TEXT PRIMARY KEY,              -- UUID v4
  title       TEXT NOT NULL,
  created_at  INTEGER NOT NULL,              -- unix epoch ms
  duration_ms INTEGER NOT NULL DEFAULT 0,
  audio_path  TEXT,
  source      TEXT NOT NULL,                 -- 'mic' | 'system' | 'mix' | 'import'
  language    TEXT NOT NULL,                 -- 'ja' | 'en' | 'auto'
  model       TEXT NOT NULL,
  status      TEXT NOT NULL,                 -- 'recording' | 'transcribing' | 'done' | 'error' | 'interrupted'
  error_message TEXT,
  drop_count  INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS segments (
  id         INTEGER PRIMARY KEY AUTOINCREMENT,
  session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
  start_ms   INTEGER NOT NULL,
  end_ms     INTEGER NOT NULL,
  text       TEXT NOT NULL,
  lang       TEXT
);
CREATE INDEX IF NOT EXISTS idx_segments_session ON segments(session_id, start_ms);

CREATE TABLE IF NOT EXISTS schema_meta (
  key   TEXT PRIMARY KEY,   -- 'schema_version' 等
  value TEXT NOT NULL
);
```
- start_ms / end_ms は**セッション先頭(=WAV先頭)からの経過ミリ秒**。一時停止中は時間をカウントしないため、WAV上の位置と常に一致する。

### 3.2 settings.json
```json
{
  "default_model": "medium-q5_0",
  "language": "ja",
  "gpu_mode": "auto",
  "mic_device": null,
  "loopback_device": null,
  "mic_gain": 0.7,
  "system_gain": 0.7,
  "vad_threshold_db": -40,
  "onboarding_done": false,
  "sound_check_recommended": true
}
```
`null` はOSデフォルトデバイス。読み書きは Rust 側のみ。

### 3.3 TypeScript 型(lib/types.ts、Rust側 serde 構造体と1:1。全構造体に `#[serde(rename_all = "camelCase")]`)
```ts
export type Source = 'mic' | 'system' | 'mix' | 'import';
export type Language = 'ja' | 'en' | 'auto';
export type SessionStatus = 'recording' | 'transcribing' | 'done' | 'error' | 'interrupted';
export type GpuMode = 'auto' | 'force_cpu' | 'force_gpu';

export interface Session {
  id: string; title: string; createdAt: number; durationMs: number;
  audioPath: string | null; source: Source; language: Language;
  model: string; status: SessionStatus; errorMessage: string | null; dropCount: number;
}
export interface Segment { id: number; sessionId: string; startMs: number; endMs: number; text: string; lang: string | null; }
export interface AudioDevice { id: string; name: string; isDefault: boolean; }

export interface ModelInfo {
  name: string; fileName: string; sizeBytes: number;
  downloaded: boolean;        // ファイル存在 かつ manifestサイズ一致
  verified: boolean;          // SHA-256 一致確認済み
  corrupted: boolean;         // サイズ or SHA 不一致を検出
  usable: boolean;            // §6.2.1 のポリシー判定結果(UI・開始時チェックはこれを見る)
  origin: 'app' | 'manual' | null;  // manifestに記録されたDL主体。手動配置は 'manual'
  recommended: boolean; description: string;
}

export interface SystemInfo {
  appVersion: string;
  compiledGpuSupport: boolean;
  requestedBackend: GpuMode;
  activeBackend: 'gpu' | 'cpu' | 'none';    // none = モデル未ロード
  gpuErrorMessage: string | null;
  modelsDir: string; dataDir: string;
}

export interface SoundCheckResult {
  micPeakDb: number | null;
  systemPeakDb: number | null;
  wavPath: string;   // Rust側の絶対パス。フロントは必ず convertFileSrc(wavPath) して再生する
  warnings: string[];
}

export interface ImportFileResult {
  path: string;
  ok: boolean;
  sessionId: string | null;      // ok=true のとき非null
  errorCode: string | null;      // ok=false のとき AppError code
  errorMessage: string | null;
}
```

### 3.4 起動時復旧(lib.rs、migration直後に実行)
1. status='recording' の残留セッション → 'interrupted' に更新、error_message='recording interrupted by app shutdown'。WAVヘッダ修復(§5.6)→ duration_ms 更新。
2. status='transcribing' の残留セッション → メモリ上のジョブキューは失われているため**自動再開はしない**。'interrupted' に更新、error_message='transcription interrupted by app shutdown'。
3. UI: interrupted セッションの詳細画面に「処理が中断されました。再文字起こしできます」バナー+再文字起こしボタンを表示(音声自体は保全されている)。

---

## 4. Tauri コマンド仕様(commands.rs)

すべて `Result<T, AppError>`。AppError は `{ code: string, message: string }`。lib/api.ts に全コマンドの型付きラッパーを実装。

| コマンド | 引数 | 戻り値 | 説明 |
|---|---|---|---|
| `get_system_info` | – | `SystemInfo` | |
| `list_audio_devices` | – | `{ inputs: AudioDevice[], outputs: AudioDevice[] }` | |
| `get_settings` / `update_settings` | – / `Partial<Settings>` | `Settings` | |
| `get_models` | – | `ModelInfo[]` | usable 判定は §6.2.1 |
| `download_model` / `cancel_download` / `delete_model` | `name` | `()` | |
| `verify_model` | `name` | `ModelInfo` | SHA再計算+HF API照合。手動配置モデルを usable 化する唯一の経路 |
| `run_sound_check` | `{ source, micDevice?, loopbackDevice?, durationMs?=5000 }` | `SoundCheckResult` | 実行中 `soundcheck://level` |
| `start_recording` | `{ source(≠import), language, model, micDevice?, loopbackDevice? }` | `sessionId` | モデルが usable でなければ `MODEL_NOT_FOUND` / `MODEL_CORRUPTED` / `MODEL_UNVERIFIED` |
| `pause_recording` / `resume_recording` | – | `()` | |
| `stop_recording` | – | `Session` | **§4.1** |
| `get_recording_state` | – | `{ active, sessionId?, paused, elapsedMs }` | |
| `get_sessions` / `get_session` / `rename_session` / `delete_session` | | | |
| `retranscribe_session` | `id, language, model` | `()` | segments削除→status='transcribing'→§6.4 バッチ経路 |
| `import_files` | `{ paths: string[], language, model, forceUnknownDuration?: boolean }` | **`ImportFileResult[]`** | ファイルごとに検証(§6.5)。失敗ファイルは ok=false で返し、成功ファイルの処理は継続。全滅でもコマンド自体はOkで返る |
| `cancel_transcription` | `sessionId` | `()` | チャンク境界で停止 |
| `export_session` | `{ sessionId, format, path }` | `()` | |

### 4.1 stop_recording のセマンティクス(厳守)
```
1. Mixer/キャプチャ停止 → Segmenter flush(SPEECH中なら最終チャンクをrtキューへ)
2. WAV finalize、duration_ms・drop_count をDBへ確定
3. JobTracker(§6.3.2)の pending_job_count(該当session) > 0 なら status='transcribing'、0なら 'done'
4. recording_active=false に設定し、即座に Session を返す。★Whisper の完了を待ってはならない★
5. 以後の done 遷移は JobTracker が §6.3.2 の規則で行い session://status を emit
```
フロントは返却 Session で `/session/:id` へ即遷移する。

### 4.2 イベント(Rust → Frontend)
| イベント名 | payload | 頻度/契機 |
|---|---|---|
| `recording://level` | `{ mic, system }` 0.0–1.0 | 100ms、録音中 |
| `recording://elapsed` | `{ elapsedMs }` | 500ms、録音中 |
| `recording://drops` | `{ dropCount }` | ドロップ検出時 |
| `soundcheck://level` | `{ mic, system }` | 100ms、テスト中 |
| `transcript://segment` | `Segment`(DB insert済み・重複除去通過後) | segment確定ごと |
| `transcript://busy` | `{ sessionId, pending }` | rtキュー滞留数変化時 |
| `session://status` | `{ sessionId, status, message? }` | 状態遷移時 |
| `import://progress` | `{ sessionId, progress }` | チャンク完了ごと |
| `model://progress` | `{ name, downloadedBytes, totalBytes }` | 250ms |
| `model://done` / `model://error` | `{ name, message? }` | |

### 4.3 AppError code 一覧(error.rs に定数化)
`MODEL_NOT_FOUND`, `MODEL_CORRUPTED`, `MODEL_UNVERIFIED`, `MODEL_ALREADY_DOWNLOADING`, `DOWNLOAD_FAILED`, `VERIFY_FAILED`, `DEVICE_NOT_FOUND`, `DEVICE_LOST`, `ALREADY_RECORDING`, `NOT_RECORDING`, `SOUND_CHECK_BUSY`, `DECODE_FAILED`, `FILE_TOO_LARGE`, `AUDIO_TOO_LONG`, `DURATION_UNKNOWN`, `DISK_FULL`, `WHISPER_GPU_UNAVAILABLE`, `WHISPER_ERROR`, `IO_ERROR`, `DB_ERROR`, `CANCELED`

---

## 5. 音声パイプライン詳細(Rust)

### 5.1 デバイス列挙(devices.rs)
- cpal `Host`(WASAPI)の `input_devices()` をマイク、`output_devices()` をループバック候補として列挙。id はデバイス名文字列。
- 録音/テスト開始時に名前照合し、見つからなければ `DEVICE_NOT_FOUND`(message に候補一覧)。**黙ってデフォルトにフォールバックしない**。`null` 設定時のみ default デバイスを使用。

### 5.2 バッファプール(buffer_pool.rs)と callback 規約
```rust
pub struct BufferPool { free_rx: Receiver<Box<[f32; CHUNK]>>, free_tx: Sender<Box<[f32; CHUNK]>> }
pub struct AudioPacket { pub buf: Box<[f32; CHUNK]>, pub len: usize }
```
- 録音/テスト開始時にストリームごと **64個**(CHUNK=4096、約85ms@48kHz)を事前確保。
- callback: 空きバッファ `try_recv` → f32 正規化コピー(超過分は複数パケット)→ `try_send`。
- 枯渇/満杯時はデータ破棄+`drop_count.fetch_add(frames)`(AtomicU64)。**callbackからのログ・emit禁止**。Mixer が定期的に監視し `recording://drops` を emit。
- Mixer は消費後バッファを `free_tx` へ返却。callback 内の処理は「コピーと lock-free チャネル操作のみ」。

### 5.3 キャプチャ(capture.rs)
- 抽象化: 以下の trait を定義し、実装を隔離する。
  ```rust
  pub trait AudioCapture: Send {
      fn start(&mut self, tx: Sender<AudioPacket>, pool: BufferPool, err_cb: ...) -> Result<StreamMeta>; // StreamMeta = { sample_rate, channels }
      fn stop(&mut self);
  }
  pub struct CpalMicCapture { ... }        // 入力デバイス
  pub struct CpalLoopbackCapture { ... }   // 出力デバイスへの build_input_stream(WASAPIループバック)
  ```
- **cpal loopback が実装検証で不可だった場合、その場で wasapi crate に書き換えない。** trait 実装 `WasapiLoopbackCapture` を**別コミット**(§15 コミット22)として追加する。パケット/プールのインターフェースは維持。
- 無音時に callback が来ない環境がある(WASAPIループバック既知挙動)→ Mixer の壁時計駆動+無音補完で吸収。
- エラーコールバック: `DEVICE_LOST` として録音を自動停止(§4.1 経路)。データ保全。

### 5.4 リサンプル(resample.rs)・ミキサー(mixer.rs)
- 多ch→モノラル: 全ch平均。rubato `SincFixedIn<f32>` でネイティブ→**16000Hz**。ストリームごと1インスタンス。
- Mixer は**壁時計基準 20ms(320サンプル@16k)ティック**のループスレッド:
  1. 各ソースのジッタバッファ(最大200ms)から取り出し、不足は無音補完(→片方停止でももう片方継続)
  2. `y = clamp(mic*mic_gain + sys*system_gain, -1.0, 1.0)`(単一ソースはパススルー+ゲイン)
  3. 出力: WAV / Segmenter / 100msごとソース別RMS→`recording://level`
- 一時停止中はティック停止(WAV位置と経過時間の一致維持)。mic-system 同期はベストエフォート(厳密同期は Phase 2)。

### 5.5 Segmenter / 簡易VAD(segmenter.rs)
- フレーム30ms(480サンプル)。`db = 20*log10(rms+1e-9)`、`speech = db > vad_threshold_db`(既定 -40dB)。
- IDLE: speech到来で SPEECH へ、**プリロール300ms**を先頭に含める。
- SPEECH: 無音 **700ms** でチャンク確定。**最大8秒**で強制確定し、次チャンク先頭に**600msオーバーラップ**(話者が連続発話しても初回セグメント表示が最大8秒+推論時間に収まるようにするため。batch側の15秒上限とは独立)。
- チャンク確定時、`TranscribeJob`(§6.3.1。valid範囲: プリロール/オーバーラップを除いた実区間)を rt キューへ。**300ms未満は破棄**。
- stop 時、SPEECH 中なら現在バッファを最終チャンクとして送出。

### 5.6 WAV(wav.rs)
- **16kHz / mono / 16bit PCM**。hound ストリーム書き込み、stop で finalize。
- `repair_wav(path) -> Result<u64>`: RIFF/data サイズを実ファイルサイズから逆算して修復、長さを返す。
- 書き込みエラーは録音自動停止+status='error'(IO_ERROR)。

### 5.7 音声テスト / サウンドチェック(sound_check.rs)
- `run_sound_check`(async): 指定ソース構成で §5.2〜5.4 と同一パイプラインを起動し、既定5秒 `soundcheck/test.wav` へ録音。100msごと `soundcheck://level`。
- 完了時にソース別ピークdBを計測し警告生成(ピーク<-60dB: ほぼ無音 / >-1dB常時: 過大入力)。
- `wavPath` は**絶対パス**。フロントは `convertFileSrc(wavPath)` で再生。
- 多重実行は `SOUND_CHECK_BUSY` / `ALREADY_RECORDING`。
- UI: Record開始前パネルに常設。`sound_check_recommended=true` 中は推奨バナー、警告なし完了で false に更新。

---

## 6. Whisper 統合・モデル管理

### 6.1 モデルカタログ(models.rs にハードコード)
DL元: `https://huggingface.co/ggerganov/whisper.cpp/resolve/main/{fileName}`

| name | fileName | 目安サイズ | UI表示 |
|---|---|---|---|
| tiny | ggml-tiny.bin | 75 MB | 動作確認用 |
| base | ggml-base.bin | 142 MB | |
| small | ggml-small.bin | 488 MB | 低スペックCPU向け |
| medium-q5_0 | ggml-medium-q5_0.bin | 539 MB | **バランス推奨(Onboarding既定選択)** |
| medium | ggml-medium.bin | 1.53 GB | 高精度・重い |
| large-v3-turbo | ggml-large-v3-turbo.bin | 1.62 GB | 高精度・高速(GPU推奨) |
| large-v3 | ggml-large-v3.bin | 2.95 GB | 最高精度(GPU推奨) |

- 正確な `sizeBytes` と SHA-256 は、DL開始時に HF API `https://huggingface.co/api/models/ggerganov/whisper.cpp/tree/main` から取得(各エントリの `size` と `lfs.oid`)。`models/manifest.json` にキャッシュ。

### 6.2 ダウンロードと検証
- reqwest streaming → `.part` へ書き込みつつ **sha2 でインクリメンタルハッシュ**。250msごと `model://progress`。
- 完了時: サイズ一致 かつ SHA一致 → リネーム、manifest に `{fileName, sizeBytes, sha256, verified:true, origin:"app"}` 記録、`model://done`。不一致 → .part削除、`model://error`(VERIFY_FAILED)。キャンセル → .part削除。
- HF API 取得失敗時: DLは続行し、完了時に manifest へ `{sizeBytes: 実サイズ, sha256: 計算値, verified:false, origin:"app"}` を記録(後で `verify_model` がHF照合を再試行できる)。
- 起動ごとのフルハッシュ再計算は行わない。`verify_model` で手動再検証(HF APIから期待値取得→照合→manifest更新)。

#### 6.2.1 モデル使用可否ポリシー(usable 判定)
| 状態 | usable | UI表示 |
|---|---|---|
| ファイル無し | false | 未DL |
| corrupted(manifestサイズ/SHA不一致) | false | 破損 → 再DL促し |
| origin='app'、verified=true | **true** | DL済 |
| origin='app'、verified=false、サイズがmanifest記録と一致 | **true**(初回使用時に「未検証モデルです」警告トーストを1回表示) | 未検証 |
| origin='manual'(manifestにエントリが無いファイル) | **false**(`verify_model` 成功で usable 化) | 手動配置・未検証 → 「検証」促し |
- `start_recording` / `import_files` / `retranscribe_session` は usable=false のモデルを `MODEL_NOT_FOUND` / `MODEL_CORRUPTED` / `MODEL_UNVERIFIED` で拒否する。
- UI のモデル選択肢は usable=true のみ活性。

### 6.3 TranscribeWorker(whisper/worker.rs)

- 状態: `Option<(model_name, WhisperContext)>`。ジョブのモデルが異なる場合のみ再ロード(gpu_mode 準拠、成否/バックエンドを SystemInfo に反映)。
- 推論パラメータ(rt/batch共通):
  ```rust
  let mut p = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
  p.set_language(...); p.set_translate(false); p.set_no_context(true);
  p.set_suppress_blank(true); p.set_token_timestamps(false);
  p.set_n_threads(min(物理コア数, 8));
  p.set_initial_prompt(同一セッションの直前確定テキスト末尾200文字);
  // batchジョブのみ: abort callback で recording_active を監視(§1.2)
  ```

#### 6.3.1 TranscribeJob とオーバーラップ重複除去(whisper/jobs.rs + whisper/mod.rs)
```rust
pub struct TranscribeJob {
    pub session_id: String,
    pub kind: JobKind,             // Rt | Batch
    pub audio: Vec<f32>,           // 16kHz mono
    pub chunk_start_ms: u64,       // audio先頭のセッション内時刻(プリロール/オーバーラップ含む)
    pub valid_start_ms: u64,       // このジョブが「結果を所有する」範囲
    pub valid_end_ms: u64,
    pub language: Language,
    pub model: String,
    pub canceled: Arc<AtomicBool>,
}
```
- オーバーラップ(rt強制確定の600ms / batch強制カットの1秒)とプリロールは**Whisperの認識安定化のためだけ**に使い、**DB保存対象は valid 範囲内のsegmentに限定**する:
  1. whisper結果の各segmentを絶対時刻に変換(t0/t1×10 + chunk_start_ms)。
  2. segment の中心時刻が `[valid_start_ms, valid_end_ms)` の外なら**破棄**(前チャンク優先の原則)。
  3. segmentが valid 境界をまたぐ場合、時刻のみ valid 範囲に clamp し、text は分割しない。
- **類似重複抑制**: DB insert 前に、同一セッションの直近保存segmentと時間が重なり(start_ms が直近segmentの end_ms より前)、かつテキストが正規化後(trim・空白除去)に一致または一方が他方を包含する場合、後続segmentを破棄する。
- 既存の抑制も維持: 空白のみ/ノイズパターン(`(音楽)`, `[音楽]`, `[BLANK_AUDIO]`, `ご視聴ありがとうございました` 等の正規表現定数)一致は破棄、同一テキスト3連続は3回目以降破棄。
- 通過したsegmentのみ DB insert → `transcript://segment` emit。

#### 6.3.2 JobTracker と完了判定(race排除)
```rust
pub struct JobTracker {
    pending: Mutex<HashMap<String /*session_id*/, u32>>,
    cancel_flags: Mutex<HashMap<String, Arc<AtomicBool>>>,
}
```
- ジョブ投入時に `pending[session] += 1`。ジョブが**成功・破棄・エラー・キャンセルスキップのいずれで終わっても必ず -1**(deferred 退避は「未完了」なので減らさない)。
- `pending[session] == 0` かつ その session が録音中でない とき**のみ** status='done'(エラー発生済みなら 'error' を維持)に更新し `session://status` を emit。この判定は decrement と同一クリティカルセクション内で行う。
- `cancel_transcription` は cancel_flags を立てる。ワーカーはチャンク処理前にチェックしスキップ(スキップも -1)。

### 6.4 バッチ処理のチャンク分割(import.rs 内 splitter)
**音声全体を1ジョブにすることを禁止。チャンクは最大15秒。**
1. デコード済み 16kHz mono f32 全体に §5.5 と同一パラメータのVADをオフライン実行し、無音(700ms以上)区間の中点を「カット候補」として列挙。
2. 先頭から貪欲にチャンク構成: **15秒**を超えない範囲で最も遠いカット候補まで。15秒以内に候補が無ければ15秒で強制カットし、次チャンク先頭に**1秒オーバーラップ**(valid_start は前チャンクの valid_end に一致させる)。
3. 各チャンクを `TranscribeJob { kind: Batch, ... }` として順次 batch キューへ投入(JobTracker +1)。
4. 進捗: チャンク完了ごと `import://progress`。**正常完了したチャンクの valid 音声時間のみを加算**して算出する(abort→deferred されたチャンクや破棄されたチャンクでは progress を進めない。二重カウント防止)。
5. 完了/キャンセルの status 遷移は JobTracker(§6.3.2)が行う。
- 再文字起こしも同一経路。

### 6.5 インポート制限と前処理(import.rs)
ファイルごとに以下を順にチェックし、失敗は `ImportFileResult{ ok:false, errorCode }` として結果配列に入れる(他ファイルは継続):
1. 拡張子 wav/mp3/m4a/aac/flac/ogg 以外 → `DECODE_FAILED`
2. サイズ > **2GB** → `FILE_TOO_LARGE`
3. duration probe: > **3時間** → `AUDIO_TOO_LONG` / 取得不能かつ `forceUnknownDuration=false` → `DURATION_UNKNOWN`(フロントが確認ダイアログ→trueで再呼び出し。デコード中に3時間相当を超えたら打ち切り `AUDIO_TOO_LONG`)
4. 見込みWAVサイズ(duration_s×32000、不明時3時間見積り)が空き容量×0.9 超 → `DISK_FULL`
- 通過 → デコード → 16kHz mono → `recordings/{session_id}.wav` 保存 → §6.4。title=ファイル名、source='import'。複数ファイル直列。

---

## 7. エクスポート(export.rs)
- **txt**: segment text を改行連結。
- **srt**: 連番 / `HH:MM:SS,mmm --> HH:MM:SS,mmm` / テキスト / 空行。
- **md**: タイトル+メタ(日時/長さ/モデル/言語)+ `**[MM:SS]** テキスト` 行。
- UTF-8(BOMなし)、LF。時刻は segments の start_ms/end_ms から生成。

---

## 8. フロントエンド仕様

### 8.1 ルーティング
| path | page |
|---|---|
| `/onboarding` | Onboarding(`onboarding_done=false` なら強制リダイレクト) |
| `/` | Library |
| `/record` | Record |
| `/session/:id` | SessionDetail |
| `/settings` | Settings |

左サイドバー(常設、幅220px): ロゴ / 「新規録音」主ボタン / ライブラリ / 設定。録音中は下部に赤ドット+経過時間(クリックで /record)。

### 8.2 各画面

**Onboarding**: ようこそ → モデル選択(medium-q5_0 既定・推奨バッジ、medium「高精度・重い(1.5GB)」、large系「GPU推奨」、進捗bytes・キャンセル・失敗時再試行・スキップ可)→ 言語既定 → 完了。

**Library(`/`)**: ヘッダ右「インポート」(dialog→`import_files`→**ImportFileResult[] のうち ok=false をファイル名+理由付きでトースト/一覧表示**)。セッションカード(新しい順): タイトル/日時/長さ/ソースアイコン/statusバッジ(transcribing=スピナー+進捗、error/interrupted=警告色、drop_count>0=欠落注意アイコン)。ホバーでリネーム・削除(確認)。空状態あり。

**Record 開始前**: ソースセグメントコントロール / デバイス・言語・モデル(usable のみ活性。未検証は「未検証」注記付きで活性)/ **SoundCheck パネル**(テスト実行→ライブレベル→結果・警告・テスト音声再生。推奨バナー)/ 録音開始ボタン。

**Record 録音中**: 経過時間(大・tabular-nums)、mic/system レベルメーター、一時停止/再開/停止。下部ライブトランスクリプト(逐次追記、自動スクロール解除+「最新へ」、busy pending>0 で「認識中…」)。`recording://drops` で欠落トースト(1回)。停止→返却Sessionで即 `/session/:id` へ。

**SessionDetail**: タイトル(インライン編集)、メタ行、エクスポート/再文字起こし/削除。**interrupted 時は「処理が中断されました。再文字起こしできます」バナー**。AudioPlayer(`convertFileSrc(audioPath)`、シーク、速度1.0/1.25/1.5/2.0)。TranscriptView(再生位置ハイライト、クリックシーク、全文コピー、transcribing中の逐次追記)。再文字起こしモーダル、ExportDialog(3形式→save dialog→完了トースト+フォルダを開く)。

**Settings**:
1. モデル: 一覧(状態バッジ: 未DL / DL済 / 未検証 / 手動配置・未検証 / 破損。DL・キャンセル・削除・**検証**ボタン、進捗)。デフォルトモデル選択
2. 音声: 既定デバイス2種、ゲイン2本、VAD閾値(-60〜-20dB)
3. 文字起こし: 既定言語、GPUモード3択(compiledGpuSupport=false で無効化+「CPU版ビルドです」注記)
4. 情報: バージョン、バックエンド表示(「GPU (Vulkan)」/「CPU — GPU初期化失敗: {gpuErrorMessage}」)、データフォルダ+開く

### 8.3 状態管理・実装規約
- Zustand 4store。**イベント購読は lib/events.ts の initEventListeners() に一元化**、Appマウント時1回(StrictMode二重実行はモジュールスコープのフラグでガード)。
- Tailwind、デザイントークンは §8.4 の確定値を theme.extend に設定。Inter + Noto Sans JP **ローカルバンドル**(@fontsource 可。**Google Fonts 等のCDN参照は禁止** — 完全ローカルアプリの方針に反する)。数値 tabular-nums。トースト/ダイアログ自作。`<form>` submit 禁止。

### 8.4 デザイントークン(確定値)
デザインは**ライト基調・シック&シンプル、アクセントは深い赤1色**で確定。`design_token.md` を参考に `tailwind.config.js` の `theme.extend` を設定する。

**使用ルール(厳守)**
- 時間表示が主役: 録音中経過時間は `time-lg`、リスト/タイムスタンプは `tabular-nums`。
- 罫線は常に `line`(8%)、ボタン枠のみ `line-strong`(14%)。角丸は chip 6 / btn 7 / card 8 / panel 12。
- **アクセント(#C4453F)の使用箇所を限定**: RECドット・主アクションボタン・「文字起こし中」バッジ・再生位置ハイライトのみ。他で使わない。
- 状態色: 完了=無彩色バッジ(`elevate` 地)、注意(interrupted / 未検証 / 欠落)= `warn`/`warn-soft`、エラー・破損= `accent`/`accent-soft`。
- モーションは `rec-pulse`(RECドット)と `seg-in`(新規セグメント追記)の2つのみ。

### 8.5 デザインモック参照と差分
Claude Design 製のHTMLモック(`Sokki.html`)を**見た目の参照**として用いる。ただし:
- **モックのコード(バンドルJS/HTML)は一切流用しない。** コンポーネントは本仕様に従い React + Tailwind で新規実装し、視覚デザインのみ踏襲する。
- モックはフォントを Google Fonts CDN から読んでいるが、実装ではローカルバンドルする(§8.3)。
- **モックに存在しない画面/状態**(v1.0時代のデザインのため)は、§8.4 のトークンと使用ルールに従い同一の意匠で新規デザインする:
  1. SoundCheck パネル(音声テスト: 実行ボタン、ライブレベル、結果・警告、テスト音声再生)
  2. モデル状態バッジ「未検証」「手動配置・未検証」「破損」(warn / accent のソフト地)
  3. interrupted セッションの「処理が中断されました。再文字起こしできます」バナー(warn-soft 地)
  4. インポートの per-file 失敗一覧表示(ImportFileResult)
  5. GPUモード3択と「CPU版ビルドです」注記

---

## 9. Tauri 設定

```jsonc
{
  "identifier": "com.sokki.app",
  "productName": "Sokki",
  "app": {
    "windows": [{ "title": "Sokki", "width": 1100, "height": 720, "minWidth": 900, "minHeight": 600 }],
    "security": {
      "assetProtocol": { "enable": true, "scope": ["$APPDATA/recordings/**/*", "$APPDATA/soundcheck/**/*"] },
      "csp": "default-src 'self'; media-src 'self' asset: http://asset.localhost; style-src 'self' 'unsafe-inline'; img-src 'self' data:"
    }
  },
  "bundle": { "active": true, "targets": ["nsis"], "windows": { "nsis": { "installMode": "currentUser" } } }
}
```
- **パス規約**: 音声ファイルパス(audioPath / wavPath)は常に Rust 側で `app_data_dir()` から生成した**絶対パス**を返す。フロントは常に `convertFileSrc()` を通す。フロントでのパス組み立て禁止。
- **M3 再生ゲート(必須)**: Tauri v2 のスコープ変数 `$APPDATA` が `app_data_dir()`(=`%APPDATA%\com.sokki.app`)に解決されることを、サウンドチェックWAVと録音WAVの**実再生**で確認する。再生できない場合は `app_data_dir()` の実パスを起動ログに出力し、scope を実パスに一致するよう修正する(例: `$APPDATA/com.sokki.app/recordings/**/*`)。**この確認が取れるまで M3 の該当コミット(§15 の31・34)から先へ進んではならない。**

### capabilities/default.json
`core:default`, `dialog:default`, `opener:default`。fs 不要(フロントからFS直接アクセス禁止)。

---

## 10. エラーハンドリング / エッジケース

| ケース | 挙動 |
|---|---|
| モデルが usable でない | `MODEL_NOT_FOUND` / `MODEL_CORRUPTED` / `MODEL_UNVERIFIED` → 設定誘導モーダル |
| 録音中デバイス切断 | 自動停止(finalize経路)、status='error'(DEVICE_LOST)、データ保全 |
| 保存デバイス消失 | `DEVICE_NOT_FOUND`+候補一覧。黙ってフォールバックしない |
| DL中ネットワーク断 | `model://error`、.part削除 |
| ディスク書き込み失敗 | 録音自動停止+error |
| 強制終了(録音中) | 起動時 interrupted 化+WAV修復(§3.4) |
| 強制終了(transcribing中) | 起動時 interrupted 化、再文字起こし誘導(§3.4) |
| 重複起動 | single-instance で既存フォーカス |
| 長時間録音 | `MAX_RECORDING_MS`=3時間で自動停止、10分前警告 |
| インポート不正 | §6.5、per-file 結果で通知 |
| オーディオドロップ | drop_count 記録+トースト+注意アイコン |
| rt処理がCPUで追いつかない | キュー滞留を許容し busy 可視化(破綻させない) |
| batch実行中に録音開始 | abort callback で当該チャンク中断→deferred退避→録音終了後に再処理(§1.2) |

---

## 11. パフォーマンス要件
- リアルタイム: チャンク確定→表示まで GPU 2秒以内 / CPU(medium-q5_0, 8スレッド)チャンク実時間の1.5倍以内目安。未達でも busy 可視化で破綻しない。
- **rt割込み保証**: 録音開始時、実行中の batch チャンクは abort により即中断される。abort が使えない場合でも15秒チャンク上限により、最初のrtジョブの待ちは最大「15秒音声1チャンクの推論時間」。
- UI: セグメント5,000件で滑らか(不足時のみ仮想化)。
- 通常負荷でオーディオドロップ 0(プール64個≈5.5秒の余裕)。
- 3時間録音での WAV duration / segments / SRT 時刻一致(±100ms)。

---

## 12. マイルストーン概要

実装単位の正は **§15 コミット計画**。

| MS | 内容 | 節目基準 |
|---|---|---|
| M1 | スキャフォールド | dev起動、全ページ骨組み、get_system_info疎通 |
| M2 | DB・設定・モデル管理・Onboarding | DL+SHA検証、usable判定、Onboarding完走 |
| M3 | 録音+音声テスト | 3ソース録音・**再生ゲート通過**、サウンドチェック、復旧 |
| M4 | ジョブ基盤+バッチ文字起こし+エクスポート | 15秒チャンク、重複除去、per-file import、srt確認 |
| M5 | リアルタイム | ライブ逐次表示、stop即応答、abort preempt |
| M6 | 仕上げ・配布 | エッジケース網羅、NSIS |

---

## 13. 受け入れ基準(最終チェックリスト)

**録音・音声**
- [ ] マイクのみ / システム音声のみ / 両方 の3構成で録音・再生できる
- [ ] システム音声のみ録音で、YouTube/Zoom/Teams/ブラウザ音声のいずれかを5秒テスト録音できる
- [ ] ミックス時、片方が無音でももう片方の録音が継続する
- [ ] 保存デバイス消失時、候補一覧付きの分かりやすいエラーが出る
- [ ] 音声テストで無音ソース警告が出て、テスト音声を再生確認できる
- [ ] 通常負荷でオーディオドロップが発生しない(drop_count=0)

**文字起こし**
- [ ] 録音中、発話から数秒以内にセグメントが逐次表示される
- [ ] **バッチ処理実行中に録音を開始すると、実行中チャンクが中断(または≤15秒チャンク1回分の待ちのみ)され、rtが優先処理される**
- [ ] バッチ処理は最大15秒チャンクに分割され、キャンセルがチャンク境界で効く
- [ ] **rt強制確定(8秒)およびbatch強制カットのオーバーラップ部で、同一発話の二重セグメントが発生しない**
- [ ] 連続発話中でも、最初のセグメントが8秒+推論時間以内に表示される
- [ ] stop_recording 後、UIは即詳細画面へ遷移し、残処理は transcribing→done と遷移する(pending_job_count による判定)
- [ ] mp3 インポートが進捗表示付きで完了し、複数ファイル時に失敗ファイルが理由付きで表示される
- [ ] gpu_mode=force_cpu で必ずCPU動作、auto失敗時に理由が設定画面に表示される

**モデル**
- [ ] DL完了時にSHA-256検証され、破損は corrupted 表示で再DL誘導される
- [ ] 未検証(origin=app)モデルは警告付きで使用でき、手動配置モデルは verify_model 成功まで使用できない
- [ ] DLキャンセルで .part が残らない

**データ整合・出力**
- [ ] srt が VLC 等で読み込める
- [ ] 3時間録音の WAV duration / segments / SRT 時刻が一致(±100ms)
- [ ] 録音WAV・テストWAVが WebView から再生できる(M3再生ゲート)

**復旧・堅牢性**
- [ ] 録音中強制終了→再起動で interrupted+WAV修復され、再文字起こしできる
- [ ] **transcribing中強制終了→再起動で interrupted になり、詳細画面から再文字起こしできる**
- [ ] モデル未DL・デバイス切断・重複起動・ディスクフル(可能なら)でクラッシュせず案内が出る
- [ ] `cargo tauri build`(CPUのみ)が Vulkan SDK なしで成功し、Cargo.lock がコミットされている
- [ ] GPUビルド手順が README に記載されている

---

## 14. Phase 2(v1では実装しない。拡張を阻害しない設計にする)
- 翻訳(translations テーブル)/ 話者分離(segments.speaker 列)/ AI要約 / 全文検索(FTS5)/ トランスクリプト編集
- モデルDLレジューム / 自動アップデータ / mic-system厳密時刻同期 / 永続ジョブキュー(transcription_jobs テーブル)

---

## 15. コミット計画

**運用規則**
- 以下の順で実装し、**1項目=1コミット**。メッセージは記載の英語をそのまま使う(Conventional Commits)。
- 各コミット前に `cargo check` と `npm run build` を通す。UIを含むコミットは `npm run tauri dev` で目視確認。
- Cargo.lock / package-lock.json の変更は必ず同コミットに含める。
- 先行コミットとの整合が崩れる場合は当該コミット内で修正する(仕様が正)。

### M1: スキャフォールド

1. `chore: initialize tauri v2 project with react-ts template`
   - create-tauri-app(React+TS+Vite)。identifier=com.sokki.app、ウィンドウ設定。Cargo.lock/package-lock.json をコミット。
   - 完了条件: `npm run tauri dev` でウィンドウ表示。
2. `chore: add tailwind css with confirmed design tokens`
   - Tailwind、**§8.4 の確定トークンを theme.extend にそのまま設定**、Inter/Noto Sans JP ローカルバンドル(CDN禁止)、tabular-nums。
3. `feat: add rust error type and logging setup`
   - error.rs(thiserror、AppError→Serialize、§4.3 全code定数)、env_logger。
4. `feat: add app data directory bootstrap`
   - recordings/ models/ soundcheck/ を起動時作成。app_data_dir() の実パスを起動ログに出力(§9 検証用)。
5. `feat: add get_system_info command with stub backend fields`
   - SystemInfo 全フィールド(activeBackend='none' スタブ)。types.ts / api.ts 雛形。
6. `feat: add app shell with sidebar and router`
   - サイドバー+5ルート、各ページ見出しのみ。
7. `feat: add typed api and event wrapper modules`
   - api.ts、events.ts(initEventListeners+二重登録ガード)、format.ts。

### M2: DB・設定・モデル管理

8. `feat: add sqlite module with schema migrations`
   - db.rs: 接続(Mutex)、§3.1 スキーマ、schema_meta migration、WAL。
9. `feat: add sessions and segments dao`
   - CRUD+update_status/update_duration。in-memory DB 単体テスト。
10. `feat: add settings persistence with get and update commands`
    - settings.json(§3.2、欠損キーはデフォルト補完)、patch 更新。
11. `feat: add settings store and general settings section`
    - useSettingsStore、Settings一般+情報セクション。
12. `feat: add model catalog and manifest module`
    - カタログ定数、manifest.json 読み書き(origin 含む)、**§6.2.1 usable 判定ロジック**、get_models。判定の単体テスト(app/manual/破損/未検証の全パターン)。
13. `feat: add model download with streaming sha256 verification`
    - HF API 取得→manifestキャッシュ、streaming DL+インクリメンタルsha2、.part→リネーム、progress/done/error、API失敗時の verified=false 経路。
14. `feat: add cancel download and delete model commands`
15. `feat: add verify_model command`
    - SHA再計算+HF照合、manifest更新。手動配置モデルの usable 化経路。
16. `feat: add model manager ui in settings`
    - 一覧、状態バッジ(未DL/DL済/未検証/手動配置・未検証/破損)、DL進捗、各操作ボタン、デフォルトモデル選択。useModelStore。
17. `feat: add onboarding flow with model download step`
    - 4ステップ、medium-q5_0 既定、スキップ・再試行、強制リダイレクト。

### M3: 録音+音声テスト

18. `feat: add audio device enumeration command`
19. `feat: add fixed-size audio buffer pool`
    - §5.2。プール枯渇・drop_count の単体テスト。
20. `feat: add audio capture trait with cpal mic implementation`
    - `AudioCapture` trait+`CpalMicCapture`(名前照合、DEVICE_NOT_FOUND、f32正規化、パケット送出、エラーコールバック)。
21. `feat: add cpal wasapi loopback capture implementation`
    - `CpalLoopbackCapture`(出力デバイスへの build_input_stream)。**ここで実機検証し、cpal不可なら本コミットは trait 側の準備までで完了とする。**
22. `feat: add wasapi fallback loopback capture`(**条件付き: コミット21でcpal loopbackが不可と判明した場合のみ。可なら実施せず欠番とする**)
    - wasapi crate による `WasapiLoopbackCapture`。インターフェース維持。Cargo.toml 追加理由を README に記録。
23. `feat: add resampler to 16khz mono`
    - サイン波単体テスト付き。
24. `feat: add mixer thread with jitter buffers and wall-clock ticks`
    - §5.4。無音補完・ゲイン・クランプ・RMS・プール返却・drop監視。
25. `feat: add streaming wav writer with header repair`
    - 途中切断WAVを生成して repair_wav を検証する単体テスト付き。
26. `feat: add recording session lifecycle commands`
    - RecordingManager: start(usable/デバイス/多重検証、status='recording')、pause/resume、stop(§4.1 手順。文字起こし未結線のため即done)、get_recording_state、recording_active フラグ(AtomicBool)導入。
27. `feat: add recording level and elapsed events`
28. `feat: add recording setup panel ui`
    - ソース/デバイス/言語/モデル(usable のみ活性)、録音開始。useRecordingStore。
29. `feat: add recording in-progress ui`
    - 経過時間、LevelMeter×2、pause/stop、サイドバーインジケータ、drops トースト(Toast 最小実装)。
30. `feat: add sound check backend`
    - §5.7。run_sound_check、level イベント、ピーク計測・警告、多重ガード。
31. `feat: add sound check ui panel with playback gate verification`
    - SoundCheck コンポーネント一式。**§9 の再生ゲート検証を実施し、scope 不一致ならこのコミット内で tauri.conf.json を修正。再生確認が取れるまで先へ進まない。**
32. `feat: add startup recovery for interrupted sessions`
    - §3.4 の1・2(recording→interrupted+WAV修復、transcribing→interrupted)。
33. `feat: add session list and library page`
    - CRUDコマンド結線、SessionCard(バッジ・欠落アイコン・リネーム・削除確認)、空状態。useSessionStore。
34. `feat: add basic session detail page with audio playback`
    - メタ、AudioPlayer、interrupted バナー枠。**録音→詳細→再生(再生ゲート第2確認)をここで完了させる。**

### M4: ジョブ基盤+バッチ文字起こし+エクスポート

35. `feat: add transcription job types and job tracker`
    - jobs.rs: TranscribeJob(§6.3.1 全フィールド)、JobTracker(pending/cancel_flags、decrement+done判定の同一クリティカルセクション実装)。増減とstatus遷移の単体テスト。
36. `feat: add whisper context management with gpu mode`
    - ロード(3モード+autoフォールバック+gpuErrorMessage)、SystemInfo 実値化、compiledGpuSupport。
37. `feat: add transcribe worker thread with priority scheduler and abort support`
    - §1.2 ループ(rt優先、recording_active 停止、deferred)、batch用 abort callback(recording_active 監視、中断→deferred退避)。使用whisper-rsバージョンでのabort API可否をここで確定し、不可の場合はREADME既知の制限に記録。
38. `feat: add whisper inference params and hallucination filtering`
    - FullParams、initial_prompt 引き継ぎ、ノイズパターン・3連続破棄。フィルタ単体テスト。
39. `feat: add overlap clipping and duplicate suppression`
    - §6.3.1 の valid範囲クリップ(中心時刻判定・clamp)+類似重複抑制。合成ケース(境界またぎ/完全重複/包含)の単体テスト。
40. `feat: add audio file decoding via symphonia`
    - probe+全デコード→16k mono。wav/mp3 フィクスチャで単体テスト。
41. `feat: add import limits validation`
    - §6.5 の1-4。単体テスト。
42. `feat: add offline vad chunk splitter with 15s cap`
    - §6.4: カット候補→貪欲15秒+1秒オーバーラップ+valid範囲設定。無音なし/短尺の境界テスト。
43. `feat: add batch transcription pipeline with per-file results`
    - import_files: per-file 検証→ImportFileResult[]、セッション作成→WAV保存→チャンク投入(JobTracker)→progress→完了遷移。直列処理。
44. `feat: add cancel transcription support`
    - cancel_flags 結線、チャンク境界スキップ(-1)、UIキャンセルボタン(Libraryカード/詳細)。
45. `feat: add transcript view with playback sync`
    - ハイライト、クリックシーク、全文コピー、逐次追記。
46. `feat: add retranscribe command and modal`
47. `feat: add export to txt srt and md`
    - フォーマット単体テスト(タイムコード境界含む)。
48. `feat: add export dialog with save path picker`

### M5: リアルタイム

49. `feat: add realtime segmenter with energy vad`
    - §5.5+valid範囲計算(プリロール/オーバーラップ除外)。合成音声(無音+トーン)単体テスト。
50. `feat: wire realtime pipeline from mixer to worker`
    - Mixer→Segmenter→rtキュー(JobTracker +1)、recording_active による batch 停止&abort の実挙動確認、言語/モデル引き回し。
51. `feat: add live transcript ui with autoscroll and pending indicator`
52. `feat: add stop flush and async completion status`
    - §4.1 完全実装(flush→pending判定→即返し→JobTracker による done 遷移→即遷移UI)。
53. `test: add scheduler priority and preemption integration test`
    - batchチャンク処理中に recording_active を立て、(a) abort発動または15秒以内にrtへ切替わること、(b) deferred が録音終了後に再処理されること、(c) pending カウントが最終的に0になり done 遷移すること を検証(whisper は tiny またはモック処理関数で代替可)。

### M6: 仕上げ・配布

54. `feat: add single instance plugin`
55. `feat: add max recording duration guard`
56. `feat: add device lost auto stop handling`
    - 手動テスト手順を README に記載。
57. `feat: add empty states error badges and dialog polish`
    - 全画面の空状態・エラー表示・確認ダイアログ・トースト文言総点検(interrupted バナー含む)。
58. `feat: add gpu mode setting section`
59. `fix: audit event listeners and store updates for duplicates`
    - StrictMode二重購読・リーク・session://status の一覧/詳細反映漏れの監査。
60. `chore: configure nsis bundle target`
    - `cargo tauri build`(CPU)成功確認。
61. `docs: add readme with build and release instructions`
    - CPU/GPUビルド(Vulkan SDK)、配布手順、既知の制限(デバイス名ID、時刻同期ベストエフォート、abort不可時の15秒待ち等)、手動テスト手順、依存バージョン変更履歴。
62. `chore: verify acceptance checklist and tag v1.0.0`
    - §13 全項目の確認結果を docs/acceptance.md に記録。

---

## 16. Composer への注意事項

- Tauri は **v2 API のみ**(v1 の `tauri::api::*`・allowlist 記法禁止)。
- フロントから直接FSを触らない。パスは常にRust生成の絶対パス+`convertFileSrc()`。
- **依存バージョンは §1.3 の指定が正。勝手に上げない。Cargo.lock/package-lock.json をコミットする。**
- **audio callback 内での `Vec` 新規確保・Mutexロック・ログ・emit 禁止**(§5.2)。
- **バッチは15秒以下チャンク+abort callback 必須**(§1.2, §6.4)。音声全体1ジョブ禁止。リアルタイムは最大8秒チャンク(§5.5)。
- **デザインモック(Sokki.html)のコードは流用禁止**。§8.4 トークンで新規実装し、見た目のみ踏襲(§8.5)。
- **stop_recording で Whisper 完了を待たない**(§4.1)。done 遷移は JobTracker のみが行う。
- **オーバーラップ結果は valid 範囲でクリップ**(§6.3.1)。
- **M3 の再生ゲート(§9)を通過するまで先へ進まない。**
- コミットは §15 の順・粒度・メッセージに従い、各コミットで `cargo check` / `npm run build` を通す。