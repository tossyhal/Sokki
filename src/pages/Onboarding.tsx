import { useEffect, useMemo, useState } from "react";
import { useNavigate } from "react-router-dom";
import { useModelStore } from "../stores/useModelStore";
import { useSettingsStore } from "../stores/useSettingsStore";
import type { Language, ModelInfo } from "../lib/types";

const steps = ["ようこそ", "モデル", "言語", "完了"] as const;
const defaultModel = "medium-q5_0";
const languageOptions: Array<{ value: Language; label: string }> = [
  { value: "ja", label: "日本語" },
  { value: "en", label: "英語" },
  { value: "auto", label: "自動判別" },
];

export default function Onboarding() {
  const navigate = useNavigate();
  const [step, setStep] = useState(0);
  const [selectedModel, setSelectedModel] = useState(defaultModel);
  const [selectedLanguage, setSelectedLanguage] = useState<Language>("ja");
  const { settings, loading: settingsLoading, saving, error, load, update } = useSettingsStore();
  const {
    models,
    progressByName,
    actionByName,
    loading: modelsLoading,
    error: modelError,
    load: loadModels,
    download,
    cancel,
  } = useModelStore();

  useEffect(() => {
    void load();
    void loadModels();
  }, [load, loadModels]);

  useEffect(() => {
    if (settings) {
      setSelectedLanguage(settings.language);
      setSelectedModel(settings.defaultModel || defaultModel);
    }
  }, [settings]);

  const recommendedModel = useMemo(
    () => models.find((model) => model.name === defaultModel) ?? models.find((model) => model.recommended),
    [models],
  );
  const selectedModelInfo = models.find((model) => model.name === selectedModel);
  const modelReady = Boolean(selectedModelInfo?.usable && !selectedModelInfo.corrupted);
  const canContinueFromModel = modelReady;

  async function finish(skippedModelDownload = false) {
    await update({
      defaultModel: selectedModel,
      language: selectedLanguage,
      onboardingDone: true,
    });
    if (!skippedModelDownload) {
      await loadModels();
    }
    navigate("/", { replace: true });
  }

  return (
    <section className="mx-auto grid max-w-4xl gap-6 px-8 py-7">
      <header>
        <p className="text-meta text-ink-2">
          {step + 1} / {steps.length}
        </p>
        <h1 className="mt-1 text-h1">Sokki</h1>
        <div className="mt-4 grid grid-cols-4 gap-2">
          {steps.map((label, index) => (
            <div key={label} className="grid gap-1">
              <div className={`h-1.5 rounded-chip ${index <= step ? "bg-accent" : "bg-elevate"}`} />
              <span className={index === step ? "text-meta text-ink" : "text-meta text-ink-2"}>
                {label}
              </span>
            </div>
          ))}
        </div>
      </header>

      {error || modelError ? (
        <div className="rounded-card border border-warn bg-warn-soft px-4 py-3 text-body text-ink">
          {error ?? modelError}
        </div>
      ) : null}

      {step === 0 ? (
        <WelcomeStep onNext={() => setStep(1)} />
      ) : step === 1 ? (
        <ModelStep
          models={models}
          loading={modelsLoading}
          selectedModel={selectedModel}
          recommendedModel={recommendedModel}
          progressByName={progressByName}
          actionByName={actionByName}
          modelReady={modelReady}
          canContinue={canContinueFromModel}
          onSelect={setSelectedModel}
          onDownload={(name) => void download(name)}
          onCancel={(name) => void cancel(name)}
          onBack={() => setStep(0)}
          onSkip={() => void finish(true)}
          onNext={() => setStep(2)}
        />
      ) : step === 2 ? (
        <LanguageStep
          value={selectedLanguage}
          onChange={setSelectedLanguage}
          onBack={() => setStep(1)}
          onNext={() => setStep(3)}
        />
      ) : (
        <FinishStep
          saving={saving || settingsLoading}
          selectedModel={selectedModel}
          selectedLanguage={selectedLanguage}
          onBack={() => setStep(2)}
          onFinish={() => void finish(false)}
        />
      )}
    </section>
  );
}

function WelcomeStep({ onNext }: { onNext: () => void }) {
  return (
    <section className="grid gap-6 border-t border-line pt-6">
      <div>
        <h2 className="text-title">ローカル文字起こしを開始</h2>
        <p className="mt-2 max-w-2xl text-body text-ink-2">
          録音、音声ファイルの文字起こし、エクスポートをこのPC上で処理します。
        </p>
      </div>
      <div className="flex justify-end">
        <button
          type="button"
          onClick={onNext}
          className="h-11 min-w-32 rounded-btn bg-accent px-4 text-body font-semibold text-white shadow-accent hover:bg-accent-hover"
        >
          開始
        </button>
      </div>
    </section>
  );
}

function ModelStep({
  models,
  loading,
  selectedModel,
  recommendedModel,
  progressByName,
  actionByName,
  modelReady,
  canContinue,
  onSelect,
  onDownload,
  onCancel,
  onBack,
  onSkip,
  onNext,
}: {
  models: ModelInfo[];
  loading: boolean;
  selectedModel: string;
  recommendedModel: ModelInfo | undefined;
  progressByName: Record<string, { downloadedBytes: number; totalBytes: number | null }>;
  actionByName: Record<string, string>;
  modelReady: boolean;
  canContinue: boolean;
  onSelect: (model: string) => void;
  onDownload: (model: string) => void;
  onCancel: (model: string) => void;
  onBack: () => void;
  onSkip: () => void;
  onNext: () => void;
}) {
  const selected = models.find((model) => model.name === selectedModel);

  return (
    <section className="grid gap-6 border-t border-line pt-6">
      <div>
        <h2 className="text-title">モデル</h2>
        <p className="mt-2 text-body text-ink-2">
          推奨は {recommendedModel?.name ?? defaultModel} です。あとから設定で変更できます。
        </p>
      </div>

      <div className="grid gap-3">
        {loading ? (
          <div className="rounded-card border border-line bg-surface px-4 py-6 text-body text-ink-2">
            モデル一覧を読み込み中
          </div>
        ) : null}
        {models.map((model) => {
          const progress = progressByName[model.name];
          const action = actionByName[model.name];
          const selectedRow = selectedModel === model.name;
          const downloading = Boolean(progress);
          const busy = Boolean(action);
          const status = modelReady && selectedRow ? "選択中・使用可能" : modelStatus(model);
          return (
            <article
              key={model.name}
              className={`grid gap-3 rounded-card border px-4 py-3 ${
                selectedRow ? "border-ink bg-surface shadow-card" : "border-line bg-surface"
              }`}
            >
              <div className="flex items-start justify-between gap-4">
                <button
                  type="button"
                  onClick={() => onSelect(model.name)}
                  className="min-w-0 flex-1 text-left"
                >
                  <span className="flex flex-wrap items-center gap-2">
                    <span className="text-title text-ink">{model.name}</span>
                    {model.recommended ? (
                      <span className="rounded-chip bg-accent-soft px-2 py-0.5 text-meta font-semibold text-accent">
                        推奨
                      </span>
                    ) : null}
                    <span className="text-meta text-ink-2">{status}</span>
                  </span>
                  <span className="mt-1 block text-body text-ink-2">{model.description}</span>
                  <span className="mt-1 block text-meta text-ink-2">
                    {model.fileName} / {formatBytes(model.sizeBytes)}
                  </span>
                </button>
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
                  ) : (
                    <span className="text-body text-ink-2">
                      {model.usable && !model.corrupted ? "使用可能" : "検証が必要"}
                    </span>
                  )}
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

      {selected ? (
        <p className="text-meta text-ink-2">
          選択中: {selected.name}
          {selected.usable && !selected.corrupted ? "" : "。ダウンロードまたは検証後に次へ進めます。"}
        </p>
      ) : null}

      <div className="flex items-center justify-between gap-3 border-t border-line pt-5">
        <button
          type="button"
          onClick={onBack}
          className="h-10 rounded-btn border border-line-strong px-3 text-body text-ink hover:bg-elevate"
        >
          戻る
        </button>
        <div className="flex items-center gap-2">
          <button
            type="button"
            onClick={onSkip}
            className="h-10 rounded-btn border border-line-strong px-3 text-body text-ink hover:bg-elevate"
          >
            スキップ
          </button>
          <button
            type="button"
            onClick={onNext}
            disabled={!canContinue}
            className="h-10 rounded-btn bg-accent px-4 text-body font-semibold text-white shadow-accent hover:bg-accent-hover disabled:bg-ink-3 disabled:shadow-none"
          >
            次へ
          </button>
        </div>
      </div>
    </section>
  );
}

function LanguageStep({
  value,
  onChange,
  onBack,
  onNext,
}: {
  value: Language;
  onChange: (language: Language) => void;
  onBack: () => void;
  onNext: () => void;
}) {
  return (
    <section className="grid gap-6 border-t border-line pt-6">
      <div>
        <h2 className="text-title">言語</h2>
        <p className="mt-2 text-body text-ink-2">録音とインポートの既定言語を選びます。</p>
      </div>
      <div className="grid gap-2">
        {languageOptions.map((option) => (
          <button
            key={option.value}
            type="button"
            onClick={() => onChange(option.value)}
            className={`h-11 rounded-btn border px-4 text-left text-body ${
              value === option.value
                ? "border-ink bg-ink font-semibold text-white"
                : "border-line-strong bg-surface text-ink hover:bg-surface-2"
            }`}
          >
            {option.label}
          </button>
        ))}
      </div>
      <div className="flex items-center justify-between border-t border-line pt-5">
        <button
          type="button"
          onClick={onBack}
          className="h-10 rounded-btn border border-line-strong px-3 text-body text-ink hover:bg-elevate"
        >
          戻る
        </button>
        <button
          type="button"
          onClick={onNext}
          className="h-10 rounded-btn bg-accent px-4 text-body font-semibold text-white shadow-accent hover:bg-accent-hover"
        >
          次へ
        </button>
      </div>
    </section>
  );
}

function FinishStep({
  saving,
  selectedModel,
  selectedLanguage,
  onBack,
  onFinish,
}: {
  saving: boolean;
  selectedModel: string;
  selectedLanguage: Language;
  onBack: () => void;
  onFinish: () => void;
}) {
  return (
    <section className="grid gap-6 border-t border-line pt-6">
      <div>
        <h2 className="text-title">準備完了</h2>
        <div className="mt-4 grid gap-3 text-body">
          <SummaryRow label="既定モデル" value={selectedModel} />
          <SummaryRow label="既定言語" value={languageLabel(selectedLanguage)} />
        </div>
      </div>
      <div className="flex items-center justify-between border-t border-line pt-5">
        <button
          type="button"
          onClick={onBack}
          className="h-10 rounded-btn border border-line-strong px-3 text-body text-ink hover:bg-elevate"
        >
          戻る
        </button>
        <button
          type="button"
          onClick={onFinish}
          disabled={saving}
          className="h-10 rounded-btn bg-accent px-4 text-body font-semibold text-white shadow-accent hover:bg-accent-hover disabled:bg-ink-3 disabled:shadow-none"
        >
          {saving ? "保存中" : "完了"}
        </button>
      </div>
    </section>
  );
}

function SummaryRow({ label, value }: { label: string; value: string }) {
  return (
    <div className="grid grid-cols-[120px_minmax(0,1fr)] gap-4 border-b border-line py-2">
      <span className="text-ink-2">{label}</span>
      <span className="min-w-0 break-all text-ink">{value}</span>
    </div>
  );
}

function modelStatus(model: ModelInfo) {
  if (!model.downloaded) {
    return "未DL";
  }
  if (model.corrupted) {
    return "破損";
  }
  if (model.origin === "manual" && !model.verified) {
    return "手動配置・未検証";
  }
  if (!model.verified) {
    return "未検証";
  }
  return "DL済";
}

function languageLabel(language: Language) {
  return languageOptions.find((option) => option.value === language)?.label ?? language;
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
