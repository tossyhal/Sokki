import { useEffect } from "react";
import { usableModels, useModelStore } from "../stores/useModelStore";
import { useSettingsStore } from "../stores/useSettingsStore";
import type { GpuMode, Language, ModelInfo, SystemInfo } from "../lib/types";

const languageOptions: Array<{ value: Language; label: string }> = [
  { value: "ja", label: "日本語" },
  { value: "en", label: "英語" },
  { value: "auto", label: "自動判別" },
];

const gpuModeOptions: Array<{ value: GpuMode; label: string }> = [
  { value: "auto", label: "自動" },
  { value: "force_cpu", label: "CPU固定" },
  { value: "force_gpu", label: "GPU固定" },
];

export default function Settings() {
  const { settings, systemInfo, loading, saving, error, load, update } = useSettingsStore();
  const {
    models,
    progressByName,
    actionByName,
    loading: modelsLoading,
    error: modelError,
    load: loadModels,
    download,
    cancel,
    delete: deleteModel,
    verify,
  } = useModelStore();

  useEffect(() => {
    void load();
    void loadModels();
  }, [load, loadModels]);

  const defaultModelOptions = usableModels(models).map((model) => ({
    value: model.name,
    label: model.verified ? model.name : `${model.name} (未検証)`,
  }));
  const currentDefaultModelIsUsable = defaultModelOptions.some(
    (option) => option.value === settings?.defaultModel,
  );
  const defaultModelSelectOptions =
    settings && !currentDefaultModelIsUsable
      ? [
          {
            value: settings.defaultModel,
            label: `${settings.defaultModel} (使用不可)`,
          },
          ...defaultModelOptions,
        ]
      : defaultModelOptions;

  return (
    <section className="mx-auto grid max-w-4xl gap-6 px-8 py-7">
      <header className="flex items-center justify-between gap-4">
        <div>
          <h1 className="text-h1">設定</h1>
          <p className="mt-1 text-meta text-ink-2">録音と文字起こしの既定値</p>
        </div>
        <div className="h-6 min-w-20 text-right text-meta text-ink-2">
          {saving ? "保存中" : settings ? "保存済み" : ""}
        </div>
      </header>

      {error || modelError ? (
        <div className="rounded-card border border-warn bg-warn-soft px-4 py-3 text-body text-ink">
          {error ?? modelError}
        </div>
      ) : null}

      <section className="border-t border-line pt-5">
        <div className="mb-5 flex items-center justify-between">
          <div>
            <h2 className="text-title">一般</h2>
            <p className="mt-1 text-meta text-ink-2">新規録音とインポートで使う初期値</p>
          </div>
          <button
            type="button"
            onClick={() => {
              void load();
              void loadModels();
            }}
            className="rounded-btn border border-line-strong px-3 py-2 text-body text-ink hover:bg-elevate"
          >
            再読み込み
          </button>
        </div>

        {loading || !settings ? (
          <div className="h-52 border border-line bg-surface-2 px-4 py-3 text-body text-ink-2">
            読み込み中
          </div>
        ) : (
          <div className="grid gap-5">
            <SelectRow
              label="既定モデル"
              value={settings.defaultModel}
              options={defaultModelSelectOptions}
              disabled={modelsLoading || defaultModelOptions.length === 0}
              onChange={(defaultModel) => void update({ defaultModel })}
            />
            <SelectRow
              label="言語"
              value={settings.language}
              options={languageOptions}
              onChange={(language) => void update({ language: language as Language })}
            />
            <GpuModeRow
              value={settings.gpuMode}
              systemInfo={systemInfo}
              onChange={(gpuMode) => void update({ gpuMode })}
            />
            <RangeRow
              label="マイクゲイン"
              value={settings.micGain}
              min={0}
              max={1}
              step={0.05}
              onChange={(micGain) => void update({ micGain })}
            />
            <RangeRow
              label="システム音声ゲイン"
              value={settings.systemGain}
              min={0}
              max={1}
              step={0.05}
              onChange={(systemGain) => void update({ systemGain })}
            />
            <RangeRow
              label="VADしきい値"
              value={settings.vadThresholdDb}
              min={-60}
              max={-20}
              step={1}
              suffix="dB"
              onChange={(vadThresholdDb) => void update({ vadThresholdDb })}
            />
            <ToggleRow
              label="音声テストの推奨を表示"
              checked={settings.soundCheckRecommended}
              onChange={(soundCheckRecommended) => void update({ soundCheckRecommended })}
            />
          </div>
        )}
      </section>

      <ModelManager
        models={models}
        loading={modelsLoading}
        progressByName={progressByName}
        actionByName={actionByName}
        onDownload={(name) => void download(name)}
        onCancel={(name) => void cancel(name)}
        onDelete={(name) => void deleteModel(name)}
        onVerify={(name) => void verify(name)}
      />

      <section className="border-t border-line pt-5">
        <h2 className="text-title">情報</h2>
        <div className="mt-4 grid gap-3 text-body">
          <InfoRow label="アプリバージョン" value={systemInfo?.appVersion ?? "-"} />
          <InfoRow label="GPU対応" value={gpuSupportLabel(systemInfo?.compiledGpuSupport)} />
          <InfoRow label="要求バックエンド" value={gpuModeLabel(systemInfo?.requestedBackend)} />
          <InfoRow label="使用中バックエンド" value={backendLabel(systemInfo)} />
          <InfoRow label="データ保存先" value={systemInfo?.dataDir ?? "-"} />
          <InfoRow label="モデル保存先" value={systemInfo?.modelsDir ?? "-"} />
        </div>
      </section>
    </section>
  );
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
        onChange={(event) => onChange(event.currentTarget.value as T)}
        disabled={disabled}
        className="h-10 rounded-btn border border-line-strong bg-surface px-3 text-body text-ink outline-none hover:bg-surface-2 focus:border-ink-2"
      >
        {options.length === 0 ? (
          <option value={value}>利用できるモデルがありません</option>
        ) : null}
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

function ModelManager({
  models,
  loading,
  progressByName,
  actionByName,
  onDownload,
  onCancel,
  onDelete,
  onVerify,
}: {
  models: ModelInfo[];
  loading: boolean;
  progressByName: Record<string, { downloadedBytes: number; totalBytes: number | null }>;
  actionByName: Record<string, string>;
  onDownload: (name: string) => void;
  onCancel: (name: string) => void;
  onDelete: (name: string) => void;
  onVerify: (name: string) => void;
}) {
  return (
    <section className="border-t border-line pt-5">
      <div className="mb-5 flex items-start justify-between gap-4">
        <div>
          <h2 className="text-title">モデル</h2>
          <p className="mt-1 text-meta text-ink-2">ローカルモデルのダウンロードと検証</p>
        </div>
        <div className="h-6 min-w-20 text-right text-meta text-ink-2">
          {loading ? "読み込み中" : `${models.length}件`}
        </div>
      </div>

      <div className="grid gap-3">
        {models.length === 0 ? (
          <div className="rounded-card border border-line bg-surface px-4 py-6 text-body text-ink-2">
            モデル一覧を読み込めませんでした。
          </div>
        ) : null}
        {models.map((model) => {
          const progress = progressByName[model.name];
          const action = actionByName[model.name];
          const downloading = Boolean(progress);
          const busy = Boolean(action);
          const canVerify = model.downloaded && (!model.verified || model.corrupted || model.origin === "manual");
          return (
            <article
              key={model.name}
              className="grid gap-3 rounded-card border border-line bg-surface px-4 py-3 shadow-card"
            >
              <div className="flex items-start justify-between gap-4">
                <div className="min-w-0">
                  <div className="flex flex-wrap items-center gap-2">
                    <h3 className="text-title text-ink">{model.name}</h3>
                    {model.recommended ? (
                      <span className="rounded-chip bg-accent-soft px-2 py-0.5 text-meta font-semibold text-accent">
                        推奨
                      </span>
                    ) : null}
                    <ModelStatusBadge model={model} />
                  </div>
                  <p className="mt-1 text-body text-ink-2">{model.description}</p>
                  <p className="mt-1 text-meta text-ink-2">
                    {model.fileName} / {formatBytes(model.sizeBytes)}
                  </p>
                </div>
                <div className="flex shrink-0 items-center gap-2">
                  {downloading ? (
                    <button
                      type="button"
                      onClick={() => onCancel(model.name)}
                      className="h-9 rounded-btn border border-line-strong px-3 text-body text-ink hover:bg-elevate disabled:text-ink-3"
                      disabled={action === "cancel"}
                    >
                      {action === "cancel" ? "中断中" : "キャンセル"}
                    </button>
                  ) : !model.downloaded ? (
                    <button
                      type="button"
                      onClick={() => onDownload(model.name)}
                      className="h-9 rounded-btn bg-accent px-3 text-body font-semibold text-white shadow-accent hover:bg-accent-hover disabled:bg-ink-3 disabled:shadow-none"
                      disabled={busy}
                    >
                      {action === "download" ? "DL中" : "DL"}
                    </button>
                  ) : null}
                  {canVerify ? (
                    <button
                      type="button"
                      onClick={() => onVerify(model.name)}
                      className="h-9 rounded-btn border border-line-strong px-3 text-body text-ink hover:bg-elevate disabled:text-ink-3"
                      disabled={busy}
                    >
                      {action === "verify" ? "検証中" : "検証"}
                    </button>
                  ) : null}
                  {model.downloaded ? (
                    <button
                      type="button"
                      onClick={() => onDelete(model.name)}
                      className="h-9 rounded-btn border border-line-strong px-3 text-body text-ink hover:bg-elevate disabled:text-ink-3"
                      disabled={busy}
                    >
                      {action === "delete" ? "削除中" : "削除"}
                    </button>
                  ) : null}
                </div>
              </div>
              {progress ? (
                <div className="grid gap-1">
                  <div className="h-2 overflow-hidden rounded-chip bg-elevate">
                    <div
                      className="h-full bg-accent"
                      style={{ width: `${progressPercent(progress)}%` }}
                    />
                  </div>
                  <p className="text-meta text-ink-2">
                    {formatBytes(progress.downloadedBytes)}
                    {progress.totalBytes ? ` / ${formatBytes(progress.totalBytes)}` : ""}
                  </p>
                </div>
              ) : null}
            </article>
          );
        })}
      </div>
    </section>
  );
}

function ModelStatusBadge({ model }: { model: ModelInfo }) {
  const status = modelStatus(model);
  return (
    <span className={`rounded-chip px-2 py-0.5 text-meta font-semibold ${status.className}`}>
      {status.label}
    </span>
  );
}

function modelStatus(model: ModelInfo) {
  if (!model.downloaded) {
    return { label: "未DL", className: "bg-elevate text-ink-2" };
  }
  if (model.corrupted) {
    return { label: "破損", className: "bg-accent-soft text-accent" };
  }
  if (model.origin === "manual" && !model.verified) {
    return { label: "手動配置・未検証", className: "bg-warn-soft text-warn" };
  }
  if (!model.verified) {
    return { label: "未検証", className: "bg-warn-soft text-warn" };
  }
  return { label: "DL済", className: "bg-elevate text-ink" };
}

function formatBytes(bytes: number) {
  if (bytes >= 1024 * 1024 * 1024) {
    return `${(bytes / 1024 / 1024 / 1024).toFixed(1)}GB`;
  }
  if (bytes >= 1024 * 1024) {
    return `${Math.round(bytes / 1024 / 1024)}MB`;
  }
  if (bytes >= 1024) {
    return `${Math.round(bytes / 1024)}KB`;
  }
  return `${bytes}B`;
}

function progressPercent(progress: { downloadedBytes: number; totalBytes: number | null }) {
  if (!progress.totalBytes || progress.totalBytes <= 0) {
    return 100;
  }
  return Math.max(2, Math.min(100, (progress.downloadedBytes / progress.totalBytes) * 100));
}

function GpuModeRow({
  value,
  systemInfo,
  onChange,
}: {
  value: GpuMode;
  systemInfo: SystemInfo | null;
  onChange: (value: GpuMode) => void;
}) {
  const gpuOptionsDisabled = systemInfo?.compiledGpuSupport === false;
  const disabledValues = gpuOptionsDisabled
    ? (["auto", "force_gpu"] as const satisfies readonly GpuMode[])
    : undefined;

  return (
    <div className="grid gap-2">
      <SelectRow
        label="GPUモード"
        value={value}
        options={gpuModeOptions}
        disabledValues={disabledValues}
        onChange={onChange}
      />
      {gpuOptionsDisabled ? (
        <p className="ml-[196px] text-meta text-ink-2">
          CPU版ビルドです。GPUを使う自動判別とGPU固定は選択できません。
        </p>
      ) : null}
      {systemInfo?.gpuErrorMessage ? (
        <p className="ml-[196px] text-meta text-ink-2">
          GPU初期化失敗: {systemInfo.gpuErrorMessage}
        </p>
      ) : null}
    </div>
  );
}

function RangeRow({
  label,
  value,
  min,
  max,
  step,
  suffix,
  onChange,
}: {
  label: string;
  value: number;
  min: number;
  max: number;
  step: number;
  suffix?: string;
  onChange: (value: number) => void;
}) {
  const displayValue = suffix ? `${value}${suffix}` : Math.round(value * 100).toString();

  return (
    <label className="grid grid-cols-[180px_minmax(0,1fr)_72px] items-center gap-4">
      <span className="text-body text-ink">{label}</span>
      <input
        type="range"
        min={min}
        max={max}
        step={step}
        value={value}
        onChange={(event) => onChange(Number(event.currentTarget.value))}
        className="h-10 accent-accent"
      />
      <span className="text-right text-body tabular-nums text-ink-2">{displayValue}</span>
    </label>
  );
}

function ToggleRow({
  label,
  checked,
  onChange,
}: {
  label: string;
  checked: boolean;
  onChange: (checked: boolean) => void;
}) {
  return (
    <label className="grid grid-cols-[180px_minmax(0,1fr)] items-center gap-4">
      <span className="text-body text-ink">{label}</span>
      <input
        type="checkbox"
        checked={checked}
        onChange={(event) => onChange(event.currentTarget.checked)}
        className="h-5 w-5 accent-accent"
      />
    </label>
  );
}

function InfoRow({ label, value }: { label: string; value: string }) {
  return (
    <div className="grid grid-cols-[180px_minmax(0,1fr)] gap-4 border-t border-line pt-3 first:border-t-0 first:pt-0">
      <div className="text-ink-2">{label}</div>
      <div className="min-w-0 break-all text-ink">{value}</div>
    </div>
  );
}

function gpuSupportLabel(compiledGpuSupport: boolean | undefined) {
  if (compiledGpuSupport === undefined) {
    return "-";
  }
  return compiledGpuSupport ? "有効" : "CPU版ビルド";
}

function gpuModeLabel(gpuMode: GpuMode | undefined) {
  switch (gpuMode) {
    case "auto":
      return "自動";
    case "force_cpu":
      return "CPU固定";
    case "force_gpu":
      return "GPU固定";
    default:
      return "-";
  }
}

function backendLabel(systemInfo: SystemInfo | null) {
  if (!systemInfo) {
    return "-";
  }
  switch (systemInfo.activeBackend) {
    case "gpu":
      return "GPU (Vulkan)";
    case "cpu":
      return systemInfo.gpuErrorMessage
        ? `CPU - GPU初期化失敗: ${systemInfo.gpuErrorMessage}`
        : "CPU";
    case "none":
      return "未ロード";
    default:
      return "-";
  }
}
