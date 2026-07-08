import { useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { useNavigate } from "react-router-dom";
import { ConfirmDialog } from "../components/ConfirmDialog";
import { importFiles } from "../lib/api";
import { formatDuration } from "../lib/format";
import type { ImportFileResult, ModelInfo, Session, SessionStatus, Source } from "../lib/types";
import { usableModels, useModelStore } from "../stores/useModelStore";
import { useSessionStore } from "../stores/useSessionStore";
import { useSettingsStore } from "../stores/useSettingsStore";

export default function Library() {
  const navigate = useNavigate();
  const { settings, load: loadSettings } = useSettingsStore();
  const { models, loading: modelsLoading, error: modelError, load: loadModels } = useModelStore();
  const {
    sessions,
    loading,
    savingId,
    deletingId,
    cancelingId,
    error,
    load,
    rename,
    cancel,
    delete: deleteSession,
  } = useSessionStore();
  const [deleteTarget, setDeleteTarget] = useState<Session | null>(null);
  const [importing, setImporting] = useState(false);
  const [importResults, setImportResults] = useState<ImportFileResult[]>([]);
  const [importError, setImportError] = useState<string | null>(null);

  useEffect(() => {
    void load();
    void loadSettings();
    void loadModels();
  }, [load, loadModels, loadSettings]);

  const importModel = selectImportModel(models, settings?.defaultModel);
  const importDisabled = loading || importing || modelsLoading;

  async function handleImport() {
    setImportError(null);
    setImportResults([]);

    if (!settings) {
      setImportError("設定を読み込み中です。少し待ってから再試行してください。");
      return;
    }
    if (!importModel) {
      setImportError("使用可能なモデルがありません。設定でモデルをダウンロードまたは検証してください。");
      return;
    }

    let selected: string | string[] | null;
    try {
      selected = await open({
        multiple: true,
        directory: false,
        filters: [
          {
            name: "Audio",
            extensions: ["wav", "mp3", "m4a", "aac", "flac", "ogg"],
          },
        ],
      });
    } catch (error) {
      setImportError(errorMessage(error));
      return;
    }
    const paths = Array.isArray(selected) ? selected : selected ? [selected] : [];
    if (paths.length === 0) {
      return;
    }

    setImporting(true);
    try {
      const results = await importFiles({
        paths,
        language: settings.language,
        model: importModel.name,
      });
      setImportResults(results);
      await load();
    } catch (error) {
      setImportError(errorMessage(error));
    } finally {
      setImporting(false);
    }
  }

  return (
    <section className="grid gap-6 px-8 py-7">
      <div className="flex items-center justify-between gap-4">
        <div>
          <h1 className="text-h1">ライブラリ</h1>
          <p className="mt-1 text-meta text-ink-2">{sessions.length} 件</p>
        </div>
        <div className="flex items-center gap-2">
          <button
            type="button"
            onClick={() => void handleImport()}
            disabled={importDisabled}
            className="h-10 rounded-btn bg-accent px-4 text-body font-semibold text-white shadow-accent hover:bg-accent-hover disabled:bg-ink-3 disabled:shadow-none"
          >
            {importing ? "インポート中" : "インポート"}
          </button>
          <button
            type="button"
            onClick={() => void load()}
            disabled={loading}
            className="h-10 rounded-btn border border-line-strong px-3 text-body text-ink hover:bg-elevate disabled:text-ink-3"
          >
            更新
          </button>
        </div>
      </div>

      {error || modelError || importError ? (
        <div className="rounded-card border border-warn bg-warn-soft px-4 py-3 text-body text-ink">
          {error ?? modelError ?? importError}
          {importError && !importModel ? (
            <button
              type="button"
              onClick={() => navigate("/settings")}
              className="ml-3 h-8 rounded-btn border border-line-strong px-3 text-meta font-semibold text-ink hover:bg-elevate"
            >
              設定を開く
            </button>
          ) : null}
        </div>
      ) : null}

      {importResults.length > 0 ? <ImportResultList results={importResults} /> : null}

      {loading && sessions.length === 0 ? (
        <div className="border-t border-line py-8 text-body text-ink-2">読み込み中</div>
      ) : null}

      {!loading && sessions.length === 0 ? (
        <div className="border-t border-line py-14 text-center">
          <p className="text-title text-ink">録音はまだありません</p>
          <button
            type="button"
            onClick={() => navigate("/record")}
            className="mt-4 h-10 rounded-btn bg-accent px-4 text-body font-semibold text-white shadow-accent hover:bg-accent-hover"
          >
            新規録音
          </button>
        </div>
      ) : null}

      {sessions.length > 0 ? (
        <div className="grid gap-3 border-t border-line pt-4">
          {sessions.map((session) => (
            <SessionCard
              key={session.id}
              session={session}
              saving={savingId === session.id}
              deleting={deletingId === session.id}
              canceling={cancelingId === session.id}
              onOpen={() => navigate(`/session/${session.id}`)}
              onRename={(title) => void rename(session.id, title)}
              onCancel={() => void cancel(session.id)}
              onDelete={() => setDeleteTarget(session)}
            />
          ))}
        </div>
      ) : null}

      {deleteTarget ? (
        <ConfirmDialog
          title="セッションを削除"
          message={`「${deleteTarget.title}」を削除します。\n録音WAVと文字起こし結果も削除されます。`}
          confirmLabel="削除"
          confirming={deletingId === deleteTarget.id}
          onCancel={() => setDeleteTarget(null)}
          onConfirm={() => {
            void deleteSession(deleteTarget.id).then((deleted) => {
              if (deleted) {
                setDeleteTarget(null);
              }
            });
          }}
        />
      ) : null}
    </section>
  );
}

function ImportResultList({ results }: { results: ImportFileResult[] }) {
  const succeeded = results.filter((result) => result.ok).length;
  const failed = results.length - succeeded;

  return (
    <section className="grid gap-3 rounded-card border border-line bg-surface px-4 py-3">
      <div className="flex items-center justify-between gap-4">
        <h2 className="text-title text-ink">インポート結果</h2>
        <span className="text-meta text-ink-2">
          成功 {succeeded} / 失敗 {failed}
        </span>
      </div>
      <div className="grid gap-2">
        {results.map((result) => (
          <div
            key={`${result.path}-${result.sessionId ?? result.errorCode ?? "result"}`}
            className={`rounded-card border px-3 py-2 text-meta ${
              result.ok ? "border-line bg-surface-2 text-ink-2" : "border-warn bg-warn-soft text-ink"
            }`}
          >
            <div className="break-all font-semibold text-ink">{fileName(result.path)}</div>
            <div className="mt-1 break-words">
              {result.ok
                ? `セッション ${result.sessionId ?? "-"} を作成しました`
                : `${result.errorCode ?? "ERROR"}: ${result.errorMessage ?? "インポートに失敗しました"}`}
            </div>
          </div>
        ))}
      </div>
    </section>
  );
}

function SessionCard({
  session,
  saving,
  deleting,
  canceling,
  onOpen,
  onRename,
  onCancel,
  onDelete,
}: {
  session: Session;
  saving: boolean;
  deleting: boolean;
  canceling: boolean;
  onOpen: () => void;
  onRename: (title: string) => void;
  onCancel: () => void;
  onDelete: () => void;
}) {
  const [editing, setEditing] = useState(false);
  const [draftTitle, setDraftTitle] = useState(session.title);

  useEffect(() => {
    setDraftTitle(session.title);
  }, [session.title]);

  const commitRename = () => {
    const title = draftTitle.trim();
    if (title && title !== session.title) {
      onRename(title);
    }
    setEditing(false);
  };

  return (
    <article
      className="grid gap-3 rounded-card border border-line bg-surface px-4 py-3 hover:bg-surface-2"
      aria-busy={saving || deleting || canceling}
    >
      <div className="flex items-start justify-between gap-4">
        <button type="button" onClick={onOpen} className="min-w-0 flex-1 text-left">
          <div className="flex min-w-0 items-center gap-2">
            <StatusBadge status={session.status} />
            {session.dropCount > 0 ? (
              <span className="rounded-chip bg-warn-soft px-2 py-1 text-micro text-ink">
                欠落 {session.dropCount}
              </span>
            ) : null}
            {!session.audioPath ? (
              <span className="rounded-chip bg-warn-soft px-2 py-1 text-micro text-ink">
                音声なし
              </span>
            ) : null}
          </div>
          <p className="mt-2 truncate text-title text-ink">{session.title}</p>
          <p className="mt-1 text-meta text-ink-2">
            {formatDate(session.createdAt)} / {formatDuration(session.durationMs)} /{" "}
            {sourceLabel(session.source)} / {session.language}
          </p>
          {session.errorMessage ? (
            <p className="mt-2 break-words text-meta text-ink-2">{session.errorMessage}</p>
          ) : null}
        </button>
        <div className="flex shrink-0 items-center gap-2">
          {session.status === "transcribing" ? (
            <button
              type="button"
              onClick={onCancel}
              disabled={saving || deleting || canceling}
              className="h-9 rounded-btn border border-line-strong px-3 text-meta font-semibold text-ink hover:bg-elevate disabled:text-ink-3"
            >
              {canceling ? "停止中" : "停止"}
            </button>
          ) : null}
          <button
            type="button"
            onClick={() => setEditing((value) => !value)}
            disabled={saving || deleting || canceling}
            className="h-9 rounded-btn border border-line-strong px-3 text-meta font-semibold text-ink hover:bg-elevate disabled:text-ink-3"
          >
            名前
          </button>
          <button
            type="button"
            onClick={onDelete}
            disabled={saving || deleting || canceling}
            className="h-9 rounded-btn border border-line-strong px-3 text-meta font-semibold text-ink hover:bg-elevate disabled:text-ink-3"
          >
            削除
          </button>
        </div>
      </div>

      {editing ? (
        <div className="flex gap-2 border-t border-line pt-3">
          <input
            value={draftTitle}
            onChange={(event) => setDraftTitle(event.target.value)}
            className="h-10 min-w-0 flex-1 rounded-btn border border-line-strong bg-surface px-3 text-body text-ink outline-none focus:border-ink-2"
            autoFocus
          />
          <button
            type="button"
            onClick={commitRename}
            disabled={saving || draftTitle.trim().length === 0}
            className="h-10 rounded-btn bg-accent px-4 text-body font-semibold text-white shadow-accent hover:bg-accent-hover disabled:bg-ink-3 disabled:shadow-none"
          >
            保存
          </button>
        </div>
      ) : null}
    </article>
  );
}

function StatusBadge({ status }: { status: SessionStatus }) {
  const tone =
    status === "error"
      ? "bg-accent-soft text-accent"
      : status === "interrupted"
        ? "bg-warn-soft text-ink"
        : status === "transcribing"
          ? "bg-accent-soft text-accent"
          : "bg-elevate text-ink-2";

  return (
    <span className={`rounded-chip px-2 py-1 text-micro font-semibold ${tone}`}>
      {statusLabel(status)}
    </span>
  );
}

function statusLabel(status: SessionStatus) {
  const labels: Record<SessionStatus, string> = {
    recording: "録音中",
    transcribing: "文字起こし中",
    done: "完了",
    error: "エラー",
    interrupted: "中断",
  };
  return labels[status];
}

function sourceLabel(source: Source) {
  const labels: Record<Source, string> = {
    mic: "マイク",
    system: "システム音声",
    mix: "ミックス",
    import: "インポート",
  };
  return labels[source];
}

function formatDate(timestampMs: number) {
  return new Intl.DateTimeFormat("ja-JP", {
    year: "numeric",
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
  }).format(new Date(timestampMs));
}

function selectImportModel(models: ModelInfo[], defaultModel: string | undefined) {
  const usable = usableModels(models);
  return usable.find((model) => model.name === defaultModel) ?? usable[0] ?? null;
}

function fileName(path: string) {
  return path.split(/[\\/]/).filter(Boolean).pop() ?? path;
}

function errorMessage(error: unknown) {
  if (error && typeof error === "object" && "message" in error) {
    return String(error.message);
  }
  return String(error);
}
