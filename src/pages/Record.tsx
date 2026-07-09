import { useEffect, useRef, useState } from "react";
import { useNavigate } from "react-router-dom";
import { convertFileSrc } from "@tauri-apps/api/core";
import { formatDuration } from "../lib/format";
import { useModelStore } from "../stores/useModelStore";
import { useRecordingStore } from "../stores/useRecordingStore";
import { useSessionStore } from "../stores/useSessionStore";
import type { AudioDevice, Language, ModelInfo, Segment, SoundCheckResult, Source } from "../lib/types";

type RecordingSource = Exclude<Source, "import">;

const sourceOptions: Array<{ value: RecordingSource; label: string }> = [
  { value: "mic", label: "マイク" },
  { value: "system", label: "システム音声" },
  { value: "mix", label: "ミックス" },
];

const languageOptions: Array<{ value: Language; label: string }> = [
  { value: "ja", label: "日本語" },
  { value: "en", label: "英語" },
  { value: "auto", label: "自動判別" },
];

export default function Record() {
  const navigate = useNavigate();
  const {
    models,
    loading: modelsLoading,
    error: modelError,
    load: loadModels,
  } = useModelStore();
  const { liveSegmentsBySession } = useSessionStore();
  const {
    devices,
    state,
    levels,
    dropCount,
    soundCheckRunning,
    soundCheckLevels,
    soundCheckResult,
    setup,
    loading,
    starting,
    stopping,
    error,
    durationLimitWarning,
    load,
    setSource,
    setLanguage,
    setModel,
    setMicDevice,
    setLoopbackDevice,
    start,
    pause,
    resume,
    stop,
    runSoundCheck,
  } = useRecordingStore();

  useEffect(() => {
    void load();
    void loadModels();
  }, [load, loadModels]);

  const needsMic = setup.source === "mic" || setup.source === "mix";
  const needsSystem = setup.source === "system" || setup.source === "mix";
  const selectedModel = models.find((model) => model.name === setup.model);
  const liveSegments = state.sessionId ? (liveSegmentsBySession[state.sessionId] ?? []) : [];
  const selectedModelUsable = Boolean(selectedModel?.usable && !selectedModel.corrupted);
  const modelOptions =
    models.length > 0
      ? models.map((model) => ({
          value: model.name,
          label: modelSelectLabel(model),
        }))
      : [{ value: setup.model, label: `${setup.model} (未読込)` }];
  const disabledModels = models
    .filter((model) => !model.usable || model.corrupted)
    .map((model) => model.name);
  const canStart =
    !loading &&
    !modelsLoading &&
    !starting &&
    !state.active &&
    selectedModelUsable &&
    (!needsMic || Boolean(devices?.inputs.length)) &&
    (!needsSystem || Boolean(devices?.outputs.length));

  return (
    <section className="mx-auto grid max-w-5xl gap-6 px-8 py-7">
      <header className="flex items-center justify-between gap-4">
        <div>
          <h1 className="text-h1">新規録音</h1>
          <p className="mt-1 text-meta text-ink-2">録音ソースと文字起こし設定</p>
        </div>
        <button
          type="button"
          onClick={() => void load()}
          className="h-10 rounded-btn border border-line-strong px-3 text-body text-ink hover:bg-elevate disabled:text-ink-3"
          disabled={loading || starting}
        >
          再読み込み
        </button>
      </header>

      {error || modelError ? (
        <div className="rounded-card border border-warn bg-warn-soft px-4 py-3 text-body text-ink">
          {error ?? modelError}
        </div>
      ) : null}

      {state.active ? (
        <RecordingInProgress
          elapsedMs={state.elapsedMs}
          paused={state.paused}
          sessionId={state.sessionId}
          micLevel={levels.mic}
          systemLevel={levels.system}
          dropCount={dropCount}
          durationLimitWarning={durationLimitWarning}
          liveSegments={liveSegments}
          stopping={stopping}
          onPause={() => void pause()}
          onResume={() => void resume()}
          onStop={() => {
            void stop().then((session) => {
              if (session) {
                navigate(`/session/${session.id}`);
              }
            });
          }}
        />
      ) : (
        <section className="grid gap-6 border-t border-line pt-5">
          <div>
            <h2 className="text-title">ソース</h2>
            <div className="mt-3 grid grid-cols-3 gap-2">
              {sourceOptions.map((option) => (
                <button
                  key={option.value}
                  type="button"
                  onClick={() => setSource(option.value)}
                  className={[
                    "h-11 rounded-btn border px-3 text-body font-semibold",
                    setup.source === option.value
                      ? "border-ink bg-ink text-white shadow-card"
                      : "border-line-strong bg-surface text-ink hover:bg-surface-2",
                  ].join(" ")}
                >
                  {option.label}
                </button>
              ))}
            </div>
          </div>

          <SoundCheckPanel
            running={soundCheckRunning}
            levels={soundCheckLevels}
            result={soundCheckResult}
            disabled={loading || starting || state.active}
            onRun={() => void runSoundCheck()}
          />

          <div className="grid gap-5">
            {needsMic && devices && devices.inputs.length === 0 ? (
              <DeviceWarning message="利用できるマイクが見つかりません。Windowsの入力デバイス設定を確認してください。" />
            ) : null}
            {needsMic ? (
              <SelectRow
                label="マイク"
                value={setup.micDevice ?? ""}
                options={deviceOptions(devices?.inputs ?? [])}
                disabled={loading}
                onChange={(value) => setMicDevice(value || null)}
              />
            ) : null}
            {needsSystem && devices && devices.outputs.length === 0 ? (
              <DeviceWarning message="利用できる出力デバイスが見つかりません。システム音声録音にはWindowsの出力デバイスが必要です。" />
            ) : null}
            {needsSystem ? (
              <SelectRow
                label="システム音声"
                value={setup.loopbackDevice ?? ""}
                options={deviceOptions(devices?.outputs ?? [])}
                disabled={loading}
                onChange={(value) => setLoopbackDevice(value || null)}
              />
            ) : null}
            <SelectRow
              label="言語"
              value={setup.language}
              options={languageOptions}
              disabled={loading}
              onChange={(language) => setLanguage(language as Language)}
            />
            <SelectRow
              label="モデル"
              value={setup.model}
              options={modelOptions}
              disabled={loading || modelsLoading}
              disabledValues={disabledModels}
              onChange={setModel}
            />
            {!selectedModelUsable ? (
              <DeviceWarning message="使用可能なモデルを設定画面でダウンロードまたは検証してください。" />
            ) : null}
          </div>

          <div className="flex items-center justify-between border-t border-line pt-5">
            <div className="text-meta text-ink-2">
              {loading ? "読み込み中" : deviceSummary(devices?.inputs.length, devices?.outputs.length)}
            </div>
            <button
              type="button"
              onClick={() => void start()}
              disabled={!canStart}
              className="h-11 min-w-36 rounded-btn bg-accent px-4 text-body font-semibold text-white shadow-accent hover:bg-accent-hover disabled:bg-ink-3 disabled:shadow-none"
            >
              {starting ? "開始中" : "録音開始"}
            </button>
          </div>
        </section>
      )}
    </section>
  );
}

function DeviceWarning({ message }: { message: string }) {
  return (
    <div className="rounded-card border border-warn bg-warn-soft px-3 py-2 text-meta text-ink">
      {message}
    </div>
  );
}

function SoundCheckPanel({
  running,
  levels,
  result,
  disabled,
  onRun,
}: {
  running: boolean;
  levels: { mic: number; system: number };
  result: SoundCheckResult | null;
  disabled: boolean;
  onRun: () => void;
}) {
  const wavSrc = result ? convertFileSrc(result.wavPath) : null;
  const [playbackStatus, setPlaybackStatus] = useState<string | null>(null);

  useEffect(() => {
    setPlaybackStatus(null);
  }, [result?.wavPath]);

  return (
    <section className="grid gap-4 border-t border-line pt-5">
      <div className="flex items-start justify-between gap-4">
        <div>
          <h2 className="text-title">音声テスト</h2>
          <p className="mt-1 text-meta text-ink-2">録音前の入力レベル確認</p>
        </div>
        <button
          type="button"
          onClick={onRun}
          disabled={disabled || running}
          className="h-10 rounded-btn border border-line-strong px-3 text-body font-semibold text-ink hover:bg-elevate disabled:text-ink-3"
        >
          {running ? "テスト中" : "テスト実行"}
        </button>
      </div>

      <div className="grid grid-cols-2 gap-4">
        <LevelMeter label="マイク" value={levels.mic} />
        <LevelMeter label="システム音声" value={levels.system} />
      </div>

      {result ? (
        <div className="grid gap-3">
          <div className="grid grid-cols-3 gap-3 text-meta text-ink-2">
            <div>
              <span className="text-ink">時間</span> {formatDuration(result.durationMs)}
            </div>
            <div>
              <span className="text-ink">Mic</span>{" "}
              {result.peakMicDb === null ? "—" : `${Math.round(result.peakMicDb)}dB`}
            </div>
            <div>
              <span className="text-ink">Sys</span>{" "}
              {result.peakSystemDb === null ? "—" : `${Math.round(result.peakSystemDb)}dB`}
            </div>
          </div>
          {result.warnings.length > 0 ? (
            <div className="grid gap-1 rounded-card border border-warn bg-warn-soft px-3 py-2 text-meta text-ink">
              {result.warnings.map((warning) => (
                <div key={warning}>{warning}</div>
              ))}
            </div>
          ) : null}
          {wavSrc ? (
            <div className="grid gap-2">
              <audio
                key={wavSrc}
                controls
                src={wavSrc}
                className="h-10 w-full"
                preload="metadata"
                onLoadedMetadata={() => setPlaybackStatus("音声メタデータを読み込みました")}
                onPlay={() => setPlaybackStatus("再生を開始しました")}
                onError={(event) => {
                  const error = event.currentTarget.error;
                  const detail = error ? `code=${error.code}` : "unknown";
                  setPlaybackStatus(`音声を読み込めません: ${detail}`);
                }}
              />
              <div className="break-all text-meta text-ink-3">
                {playbackStatus ? `${playbackStatus} / ` : null}
                {result.wavPath}
                <br />
                {wavSrc}
              </div>
            </div>
          ) : null}
        </div>
      ) : null}
    </section>
  );
}

function RecordingInProgress({
  elapsedMs,
  paused,
  sessionId,
  micLevel,
  systemLevel,
  dropCount,
  durationLimitWarning,
  liveSegments,
  stopping,
  onPause,
  onResume,
  onStop,
}: {
  elapsedMs: number;
  paused: boolean;
  sessionId: string | null;
  micLevel: number;
  systemLevel: number;
  dropCount: number;
  durationLimitWarning: string | null;
  liveSegments: Segment[];
  stopping: boolean;
  onPause: () => void;
  onResume: () => void;
  onStop: () => void;
}) {
  return (
    <section className="grid gap-6 border-t border-line pt-5">
      <div className="flex items-start justify-between gap-6">
        <div>
          <div className="flex items-center gap-3">
            <span className="h-3 w-3 rounded-full bg-accent animate-rec-pulse" />
            <span className="text-title">{paused ? "一時停止中" : "録音中"}</span>
          </div>
          <p className="mt-2 max-w-xl break-all text-meta text-ink-2">{sessionId ?? "-"}</p>
        </div>
        <div className="text-right">
          <div className="text-time-lg tabular-nums text-ink">{formatDuration(elapsedMs)}</div>
          {dropCount > 0 ? (
            <div className="mt-2 rounded-chip bg-warn-soft px-2 py-1 text-meta text-ink">
              欠落 {dropCount}
            </div>
          ) : null}
        </div>
      </div>

      <div className="grid grid-cols-2 gap-4">
        <LevelMeter label="マイク" value={micLevel} />
        <LevelMeter label="システム音声" value={systemLevel} />
      </div>

      <LiveTranscriptPanel segments={liveSegments} />

      {durationLimitWarning ? (
        <div className="rounded-card border border-warn bg-warn-soft px-3 py-2 text-meta text-ink">
          {durationLimitWarning}
        </div>
      ) : null}

      <div className="flex items-center justify-end gap-3 border-t border-line pt-5">
        {paused ? (
          <button
            type="button"
            onClick={onResume}
            disabled={stopping}
            className="h-10 rounded-btn border border-line-strong px-4 text-body font-semibold text-ink hover:bg-elevate disabled:text-ink-3"
          >
            再開
          </button>
        ) : (
          <button
            type="button"
            onClick={onPause}
            disabled={stopping}
            className="h-10 rounded-btn border border-line-strong px-4 text-body font-semibold text-ink hover:bg-elevate disabled:text-ink-3"
          >
            一時停止
          </button>
        )}
        <button
          type="button"
          onClick={onStop}
          disabled={stopping}
          className="h-10 min-w-28 rounded-btn bg-accent px-4 text-body font-semibold text-white shadow-accent hover:bg-accent-hover disabled:bg-ink-3 disabled:shadow-none"
        >
          {stopping ? "停止中" : "停止"}
        </button>
      </div>
    </section>
  );
}

function LiveTranscriptPanel({ segments }: { segments: Segment[] }) {
  const [autoScroll, setAutoScroll] = useState(true);
  const endRef = useRef<HTMLDivElement | null>(null);
  const hasSegments = segments.length > 0;

  useEffect(() => {
    if (autoScroll) {
      endRef.current?.scrollIntoView({ block: "end" });
    }
  }, [autoScroll, segments.length]);

  const jumpToLatest = () => {
    setAutoScroll(true);
    requestAnimationFrame(() => endRef.current?.scrollIntoView({ block: "end" }));
  };

  return (
    <section className="grid gap-3 border-t border-line pt-5">
      <div className="flex items-center justify-between gap-3">
        <div>
          <h2 className="text-title">ライブ文字起こし</h2>
          <p className="mt-1 text-meta text-ink-2">
            {hasSegments ? `${segments.length} セグメント` : "発話を待機中"}
          </p>
        </div>
        {!autoScroll ? (
          <button
            type="button"
            onClick={jumpToLatest}
            className="h-9 rounded-btn border border-line-strong px-3 text-meta font-semibold text-ink hover:bg-elevate"
          >
            最新へ
          </button>
        ) : null}
      </div>

      <div
        onScroll={(event) => {
          const element = event.currentTarget;
          const distanceFromBottom = element.scrollHeight - element.scrollTop - element.clientHeight;
          setAutoScroll(distanceFromBottom < 32);
        }}
        className="grid max-h-64 gap-2 overflow-y-auto rounded-card border border-line bg-surface p-3"
      >
        {segments.map((segment) => (
          <div
            key={segment.id}
            className="grid grid-cols-[72px_1fr] gap-3 rounded-card border border-line bg-surface-2 px-3 py-3 animate-seg-in"
          >
            <span className="text-meta font-semibold tabular-nums text-ink-2">
              {formatTimestamp(segment.startMs)}
            </span>
            <span className="text-body text-ink">{segment.text}</span>
          </div>
        ))}
        <div className="grid grid-cols-[72px_1fr] gap-3 rounded-card border border-dashed border-line-strong bg-surface px-3 py-3">
          <span className="text-meta font-semibold text-ink-3">LIVE</span>
          <span className="inline-flex items-center gap-2 text-body text-ink-2">
            <span className="h-2 w-2 rounded-full bg-accent motion-safe:animate-rec-pulse" />
            認識中…
          </span>
        </div>
        <div ref={endRef} />
      </div>
    </section>
  );
}

function LevelMeter({ label, value }: { label: string; value: number }) {
  const db = levelToDb(value);
  const percent = dbToMeterPercent(db);

  return (
    <div className="grid gap-2">
      <div className="flex items-center justify-between text-meta">
        <span className="text-ink">{label}</span>
        <span className="tabular-nums text-ink-2">
          {db <= -60 ? "-60dB" : `${Math.round(db)}dB`}
        </span>
      </div>
      <div className="h-3 overflow-hidden rounded-chip bg-elevate">
        <div className="h-full bg-[#2F6F6D]" style={{ width: `${percent}%` }} />
      </div>
    </div>
  );
}

function levelToDb(value: number) {
  const clamped = Math.max(0, Math.min(1, value));
  if (clamped <= 0) {
    return -60;
  }
  return Math.max(-60, Math.min(0, 20 * Math.log10(clamped)));
}

function dbToMeterPercent(db: number) {
  return Math.max(0, Math.min(100, ((db + 60) / 60) * 100));
}

function SelectRow<T extends string>({
  label,
  value,
  options,
  disabled,
  disabledValues,
  onChange,
}: {
  label: string;
  value: T;
  options: Array<{ value: T; label: string }>;
  disabled?: boolean;
  disabledValues?: readonly T[];
  onChange: (value: T) => void;
}) {
  return (
    <label className="grid grid-cols-[180px_minmax(0,1fr)] items-center gap-4">
      <span className="text-body text-ink">{label}</span>
      <select
        value={value}
        disabled={disabled}
        onChange={(event) => onChange(event.currentTarget.value as T)}
        className="h-10 rounded-btn border border-line-strong bg-surface px-3 text-body text-ink outline-none hover:bg-surface-2 focus:border-ink-2 disabled:text-ink-3"
      >
        {options.map((option) => (
          <option
            key={option.value}
            value={option.value}
            disabled={disabledValues?.includes(option.value)}
          >
            {option.label}
          </option>
        ))}
      </select>
    </label>
  );
}

function deviceOptions(devices: AudioDevice[]) {
  return [
    { value: "", label: "既定デバイス" },
    ...devices.map((device) => ({
      value: device.id,
      label: device.isDefault ? `${device.name} / 既定` : device.name,
    })),
  ];
}

function modelSelectLabel(model: ModelInfo) {
  if (!model.downloaded) {
    return `${model.name} / 未DL`;
  }
  if (model.corrupted) {
    return `${model.name} / 破損`;
  }
  if (model.origin === "manual" && !model.verified) {
    return `${model.name} / 手動配置・未検証`;
  }
  if (!model.verified) {
    return `${model.name} / 未検証`;
  }
  return model.name;
}

function deviceSummary(inputCount: number | undefined, outputCount: number | undefined) {
  return `入力 ${inputCount ?? 0} / 出力 ${outputCount ?? 0}`;
}

function formatTimestamp(timestampMs: number) {
  const totalSeconds = Math.floor(timestampMs / 1_000);
  const minutes = Math.floor(totalSeconds / 60);
  const seconds = totalSeconds % 60;
  return `${String(minutes).padStart(2, "0")}:${String(seconds).padStart(2, "0")}`;
}
