import { useEffect } from "react";
import { useRecordingStore } from "../stores/useRecordingStore";
import type { AudioDevice, Language, Source } from "../lib/types";

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

const modelOptions = ["medium-q5_0", "small-q5_0", "base-q5_0"];

export default function Record() {
  const {
    devices,
    state,
    setup,
    loading,
    starting,
    error,
    load,
    setSource,
    setLanguage,
    setModel,
    setMicDevice,
    setLoopbackDevice,
    start,
  } = useRecordingStore();

  useEffect(() => {
    void load();
  }, [load]);

  const needsMic = setup.source === "mic" || setup.source === "mix";
  const needsSystem = setup.source === "system" || setup.source === "mix";
  const canStart =
    !loading &&
    !starting &&
    !state.active &&
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

      {error ? (
        <div className="rounded-card border border-warn bg-warn-soft px-4 py-3 text-body text-ink">
          {error}
        </div>
      ) : null}

      {state.active ? (
        <section className="border-t border-line pt-5">
          <div className="flex items-center gap-3">
            <span className="h-3 w-3 rounded-full bg-accent animate-rec-pulse" />
            <div>
              <h2 className="text-title">録音中</h2>
              <p className="mt-1 text-meta text-ink-2">{state.sessionId ?? "-"}</p>
            </div>
          </div>
        </section>
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
                      ? "border-accent bg-accent text-white shadow-accent"
                      : "border-line-strong bg-surface text-ink hover:bg-surface-2",
                  ].join(" ")}
                >
                  {option.label}
                </button>
              ))}
            </div>
          </div>

          <div className="grid gap-5">
            {needsMic ? (
              <SelectRow
                label="マイク"
                value={setup.micDevice ?? ""}
                options={deviceOptions(devices?.inputs ?? [])}
                disabled={loading}
                onChange={(value) => setMicDevice(value || null)}
              />
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
              options={modelOptions.map((model) => ({ value: model, label: model }))}
              disabled={loading}
              onChange={setModel}
            />
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

function SelectRow<T extends string>({
  label,
  value,
  options,
  disabled,
  onChange,
}: {
  label: string;
  value: T;
  options: Array<{ value: T; label: string }>;
  disabled?: boolean;
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
          <option key={option.value} value={option.value}>
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

function deviceSummary(inputCount: number | undefined, outputCount: number | undefined) {
  return `入力 ${inputCount ?? 0} / 出力 ${outputCount ?? 0}`;
}
