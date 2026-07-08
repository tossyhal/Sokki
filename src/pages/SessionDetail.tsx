import { useEffect, useRef, useState } from "react";
import { convertFileSrc } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { save } from "@tauri-apps/plugin-dialog";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import { useNavigate, useParams } from "react-router-dom";
import { ConfirmDialog } from "../components/ConfirmDialog";
import { exportSession, getSegments, retranscribeSession } from "../lib/api";
import { formatDuration } from "../lib/format";
import type { ExportFormat, Language, Segment, Session, SessionStatus, Source } from "../lib/types";
import { useSessionStore } from "../stores/useSessionStore";

export default function SessionDetail() {
  const { id } = useParams();
  const navigate = useNavigate();
  const {
    selected,
    loading,
    cancelingId,
    deletingId,
    error,
    loadOne,
    cancel,
    delete: deleteSession,
  } = useSessionStore();
  const audioRef = useRef<HTMLAudioElement | null>(null);
  const [segments, setSegments] = useState<Segment[]>([]);
  const [segmentError, setSegmentError] = useState<string | null>(null);
  const [currentMs, setCurrentMs] = useState(0);
  const [showRetranscribe, setShowRetranscribe] = useState(false);
  const [retranscribeLanguage, setRetranscribeLanguage] = useState<Language>("ja");
  const [retranscribeModel, setRetranscribeModel] = useState("medium-q5_0");
  const [retranscribing, setRetranscribing] = useState(false);
  const [retranscribeError, setRetranscribeError] = useState<string | null>(null);
  const [showExport, setShowExport] = useState(false);
  const [showDeleteConfirm, setShowDeleteConfirm] = useState(false);
  const [requestedSessionId, setRequestedSessionId] = useState<string | null>(null);

  useEffect(() => {
    if (id) {
      setRequestedSessionId(id);
      void loadOne(id);
    }
  }, [id, loadOne]);

  const session = selected?.id === id ? selected : null;

  useEffect(() => {
    if (session) {
      setRetranscribeLanguage(session.language);
      setRetranscribeModel(session.model);
    }
  }, [session?.id, session?.language, session?.model]);

  useEffect(() => {
    let active = true;
    setSegments([]);
    setSegmentError(null);
    setCurrentMs(0);
    if (!id) {
      return;
    }

    void getSegments(id)
      .then((items) => {
        if (active) {
          setSegments(items);
        }
      })
      .catch((error) => {
        if (active) {
          setSegmentError(errorMessage(error));
        }
      });

    return () => {
      active = false;
    };
  }, [id]);

  useEffect(() => {
    if (!id) {
      return;
    }

    let unlisten: (() => void) | null = null;
    let disposed = false;

    void listen<Segment>("transcript://segment", (event) => {
      const segment = event.payload;
      if (segment.sessionId !== id) {
        return;
      }
      setSegments((current) => mergeSegment(current, segment));
    }).then((handler) => {
      if (disposed) {
        handler();
      } else {
        unlisten = handler;
      }
    });

    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [id]);

  return (
    <section className="grid gap-6 px-8 py-7">
      <div className="flex items-center justify-between gap-4">
        <div className="min-w-0">
          <button
            type="button"
            onClick={() => navigate("/")}
            className="mb-3 h-9 rounded-btn border border-line-strong px-3 text-meta font-semibold text-ink hover:bg-elevate"
          >
            一覧
          </button>
          <h1 className="truncate text-h1">{session?.title ?? "セッション詳細"}</h1>
        </div>
        <div className="flex shrink-0 items-center gap-2">
          {session ? (
            <button
              type="button"
              onClick={() => setShowExport(true)}
              disabled={segments.length === 0}
              className="h-10 rounded-btn border border-line-strong px-4 text-body font-semibold text-ink hover:bg-elevate disabled:text-ink-3"
            >
              エクスポート
            </button>
          ) : null}
          {session && session.status !== "recording" && session.status !== "transcribing" ? (
            <button
              type="button"
              onClick={() => {
                setRetranscribeError(null);
                setShowRetranscribe(true);
              }}
              className="h-10 rounded-btn border border-line-strong px-4 text-body font-semibold text-ink hover:bg-elevate"
            >
              再文字起こし
            </button>
          ) : null}
          {session?.status === "transcribing" ? (
            <button
              type="button"
              onClick={() => void cancel(session.id)}
              disabled={cancelingId === session.id}
              className="h-10 rounded-btn border border-line-strong px-4 text-body font-semibold text-ink hover:bg-elevate disabled:text-ink-3"
            >
              {cancelingId === session.id ? "停止中" : "文字起こし停止"}
            </button>
          ) : null}
          {session ? (
            <button
              type="button"
              onClick={() => setShowDeleteConfirm(true)}
              disabled={deletingId === session.id}
              className="h-10 rounded-btn border border-line-strong px-4 text-body font-semibold text-ink hover:bg-elevate disabled:text-ink-3"
            >
              {deletingId === session.id ? "削除中" : "削除"}
            </button>
          ) : null}
        </div>
      </div>

      {error ? (
        <div className="rounded-card border border-warn bg-warn-soft px-4 py-3 text-body text-ink">
          {error}
        </div>
      ) : null}

      {loading && !session ? (
        <div className="border-t border-line py-8 text-body text-ink-2">読み込み中</div>
      ) : null}

      {!loading && !error && !session && requestedSessionId === id ? (
        <div className="rounded-card border border-line bg-surface px-4 py-8 text-center">
          <p className="text-title text-ink">セッションが見つかりません</p>
          <button
            type="button"
            onClick={() => navigate("/")}
            className="mt-4 h-10 rounded-btn border border-line-strong px-4 text-body font-semibold text-ink hover:bg-elevate"
          >
            ライブラリへ戻る
          </button>
        </div>
      ) : null}

      {session ? (
        <>
          {session.status === "interrupted" ? (
            <div className="rounded-card border border-warn bg-warn-soft px-4 py-3 text-body text-ink">
              処理が中断されました。再文字起こしできます。
            </div>
          ) : null}
          {session.status === "error" ? (
            <div className="rounded-card border border-accent bg-accent-soft px-4 py-3 text-body text-ink">
              セッションでエラーが発生しました。必要に応じて再文字起こしできます。
            </div>
          ) : null}

          <SessionMeta session={session} />
          <AudioPlayer
            audioRef={audioRef}
            session={session}
            onTimeUpdate={(timeMs) => setCurrentMs(timeMs)}
          />
          <TranscriptView
            segments={segments}
            currentMs={currentMs}
            error={segmentError}
            liveStatus={
              session.status === "recording" || session.status === "transcribing"
                ? session.status
                : null
            }
            onSeek={(timeMs) => {
              if (audioRef.current) {
                audioRef.current.currentTime = timeMs / 1_000;
              }
            }}
          />
        </>
      ) : null}

      {session && showRetranscribe ? (
        <RetranscribeModal
          language={retranscribeLanguage}
          model={retranscribeModel}
          submitting={retranscribing}
          error={retranscribeError}
          onLanguageChange={setRetranscribeLanguage}
          onModelChange={setRetranscribeModel}
          onCancel={() => setShowRetranscribe(false)}
          onConfirm={() => {
            setRetranscribing(true);
            setRetranscribeError(null);
            void retranscribeSession({
              id: session.id,
              language: retranscribeLanguage,
              model: retranscribeModel.trim(),
            })
              .then(async () => {
                setSegments([]);
                setShowRetranscribe(false);
                await loadOne(session.id);
              })
              .catch((error) => setRetranscribeError(errorMessage(error)))
              .finally(() => setRetranscribing(false));
          }}
        />
      ) : null}

      {session && showExport ? (
        <ExportModal
          session={session}
          onCancel={() => setShowExport(false)}
          onDone={() => setShowExport(false)}
        />
      ) : null}

      {session && showDeleteConfirm ? (
        <ConfirmDialog
          title="セッションを削除"
          message={`「${session.title}」を削除します。\n録音WAVと文字起こし結果も削除されます。`}
          confirmLabel="削除"
          confirming={deletingId === session.id}
          onCancel={() => setShowDeleteConfirm(false)}
          onConfirm={() => {
            void deleteSession(session.id).then((deleted) => {
              if (deleted) {
                setShowDeleteConfirm(false);
                navigate("/");
              }
            });
          }}
        />
      ) : null}
    </section>
  );
}

function ExportModal({
  session,
  onCancel,
  onDone,
}: {
  session: Session;
  onCancel: () => void;
  onDone: () => void;
}) {
  const [format, setFormat] = useState<ExportFormat>("txt");
  const [exporting, setExporting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [exportedPath, setExportedPath] = useState<string | null>(null);
  const extension = formatExtension(format);

  const runExport = async () => {
    setExporting(true);
    setError(null);
    setExportedPath(null);
    try {
      const path = await save({
        title: "文字起こしをエクスポート",
        defaultPath: `${safeFileBase(session.title)}.${extension}`,
        filters: [{ name: formatLabel(format), extensions: [extension] }],
      });
      if (!path) {
        setExporting(false);
        return;
      }
      await exportSession({ sessionId: session.id, format, path });
      setExportedPath(path);
    } catch (error) {
      setError(errorMessage(error));
    } finally {
      setExporting(false);
    }
  };

  const revealExport = async () => {
    if (!exportedPath) {
      return;
    }
    try {
      await revealItemInDir(exportedPath);
    } catch (error) {
      setError(errorMessage(error));
    }
  };

  return (
    <div className="fixed inset-0 z-30 flex items-center justify-center bg-black/25 px-4">
      <section className="w-full max-w-md rounded-card border border-line bg-surface p-5 shadow-panel">
        <h2 className="text-title text-ink">エクスポート</h2>
        <p className="mt-1 text-meta text-ink-2">文字起こしをファイルに保存します。</p>

        <div className="mt-5 grid grid-cols-3 gap-2">
          {(["txt", "srt", "md"] as const).map((value) => (
            <button
              key={value}
              type="button"
              onClick={() => {
                setFormat(value);
                setError(null);
                setExportedPath(null);
              }}
              disabled={exporting}
              className={`h-10 rounded-btn border px-3 text-body font-semibold ${
                format === value
                  ? "border-ink-2 bg-elevate text-ink"
                  : "border-line-strong text-ink hover:bg-elevate"
              } disabled:text-ink-3`}
            >
              {value.toUpperCase()}
            </button>
          ))}
        </div>

        {exportedPath ? (
          <div className="mt-4 rounded-card border border-line bg-elevate px-4 py-3 text-body text-ink">
            <p>保存しました</p>
            <p className="mt-1 break-all text-meta text-ink-2">{exportedPath}</p>
          </div>
        ) : null}

        {error ? (
          <div className="mt-4 rounded-card border border-warn bg-warn-soft px-4 py-3 text-body text-ink">
            {error}
          </div>
        ) : null}

        <div className="mt-5 flex justify-end gap-2">
          {exportedPath ? (
            <button
              type="button"
              onClick={() => void revealExport()}
              disabled={exporting}
              className="h-10 rounded-btn border border-line-strong px-4 text-body font-semibold text-ink hover:bg-elevate disabled:text-ink-3"
            >
              保存先を開く
            </button>
          ) : null}
          <button
            type="button"
            onClick={exportedPath ? onDone : onCancel}
            disabled={exporting}
            className="h-10 rounded-btn border border-line-strong px-4 text-body font-semibold text-ink hover:bg-elevate disabled:text-ink-3"
          >
            {exportedPath ? "閉じる" : "キャンセル"}
          </button>
          {!exportedPath ? (
            <button
              type="button"
              onClick={() => void runExport()}
              disabled={exporting}
              className="h-10 rounded-btn bg-accent px-4 text-body font-semibold text-white shadow-accent hover:bg-accent-hover disabled:bg-ink-3 disabled:shadow-none"
            >
              {exporting ? "保存中" : "保存"}
            </button>
          ) : null}
        </div>
      </section>
    </div>
  );
}

function RetranscribeModal({
  language,
  model,
  submitting,
  error,
  onLanguageChange,
  onModelChange,
  onCancel,
  onConfirm,
}: {
  language: Language;
  model: string;
  submitting: boolean;
  error: string | null;
  onLanguageChange: (language: Language) => void;
  onModelChange: (model: string) => void;
  onCancel: () => void;
  onConfirm: () => void;
}) {
  const canSubmit = model.trim().length > 0 && !submitting;

  return (
    <div className="fixed inset-0 z-30 flex items-center justify-center bg-black/25 px-4">
      <section className="w-full max-w-md rounded-card border border-line bg-surface p-5 shadow-panel">
        <h2 className="text-title text-ink">再文字起こし</h2>
        <p className="mt-1 text-meta text-ink-2">既存の文字起こしを削除して再処理します。</p>

        <div className="mt-5 grid gap-4">
          <label className="grid gap-2 text-body text-ink">
            言語
            <select
              value={language}
              onChange={(event) => onLanguageChange(event.target.value as Language)}
              className="h-10 rounded-btn border border-line-strong bg-surface px-3 text-body text-ink outline-none focus:border-ink-2"
            >
              <option value="ja">日本語</option>
              <option value="en">英語</option>
              <option value="auto">自動</option>
            </select>
          </label>
          <label className="grid gap-2 text-body text-ink">
            モデル
            <input
              value={model}
              onChange={(event) => onModelChange(event.target.value)}
              className="h-10 rounded-btn border border-line-strong bg-surface px-3 text-body text-ink outline-none focus:border-ink-2"
            />
          </label>
        </div>

        {error ? (
          <div className="mt-4 rounded-card border border-warn bg-warn-soft px-4 py-3 text-body text-ink">
            {error}
          </div>
        ) : null}

        <div className="mt-5 flex justify-end gap-2">
          <button
            type="button"
            onClick={onCancel}
            disabled={submitting}
            className="h-10 rounded-btn border border-line-strong px-4 text-body font-semibold text-ink hover:bg-elevate disabled:text-ink-3"
          >
            キャンセル
          </button>
          <button
            type="button"
            onClick={onConfirm}
            disabled={!canSubmit}
            className="h-10 rounded-btn bg-accent px-4 text-body font-semibold text-white shadow-accent hover:bg-accent-hover disabled:bg-ink-3 disabled:shadow-none"
          >
            {submitting ? "開始中" : "開始"}
          </button>
        </div>
      </section>
    </div>
  );
}

function SessionMeta({ session }: { session: Session }) {
  return (
    <section className="grid gap-3 border-t border-line pt-5">
      <div className="grid grid-cols-2 gap-3 text-body md:grid-cols-4">
        <MetaItem label="状態" value={statusLabel(session.status)} />
        <MetaItem label="時間" value={formatDuration(session.durationMs)} />
        <MetaItem label="ソース" value={sourceLabel(session.source)} />
        <MetaItem label="言語" value={session.language} />
      </div>
      <div className="grid grid-cols-2 gap-3 text-body md:grid-cols-4">
        <MetaItem label="モデル" value={session.model} />
        <MetaItem label="作成" value={formatDate(session.createdAt)} />
        <MetaItem label="欠落" value={String(session.dropCount)} />
        <MetaItem label="ID" value={session.id} />
      </div>
      {session.errorMessage ? (
        <div className="rounded-card border border-warn bg-warn-soft px-3 py-2 text-meta text-ink">
          {session.errorMessage}
        </div>
      ) : null}
    </section>
  );
}

function MetaItem({ label, value }: { label: string; value: string }) {
  return (
    <div className="min-w-0">
      <p className="text-meta text-ink-2">{label}</p>
      <p className="truncate text-body text-ink">{value}</p>
    </div>
  );
}

function AudioPlayer({
  audioRef,
  session,
  onTimeUpdate,
}: {
  audioRef: React.RefObject<HTMLAudioElement | null>;
  session: Session;
  onTimeUpdate: (timeMs: number) => void;
}) {
  const audioSrc = session.audioPath ? convertFileSrc(session.audioPath) : null;
  const [playbackStatus, setPlaybackStatus] = useState<string | null>(null);
  const [speed, setSpeed] = useState(1);

  useEffect(() => {
    setPlaybackStatus(null);
  }, [session.audioPath]);

  useEffect(() => {
    if (audioRef.current) {
      audioRef.current.playbackRate = speed;
    }
  }, [audioRef, speed]);

  return (
    <section className="grid gap-3 border-t border-line pt-5">
      <div>
        <h2 className="text-title">音声</h2>
        <p className="mt-1 text-meta text-ink-2">録音WAV</p>
      </div>

      {audioSrc ? (
        <div className="grid gap-2">
          <audio
            ref={audioRef}
            key={audioSrc}
            controls
            src={audioSrc}
            className="h-10 w-full"
            preload="metadata"
            onLoadedMetadata={() => setPlaybackStatus("音声メタデータを読み込みました")}
            onTimeUpdate={(event) => onTimeUpdate(Math.floor(event.currentTarget.currentTime * 1_000))}
            onPlay={() => setPlaybackStatus("再生を開始しました")}
            onError={(event) => {
              const error = event.currentTarget.error;
              const detail = error ? `code=${error.code}` : "unknown";
              setPlaybackStatus(`音声を読み込めません: ${detail}`);
            }}
          />
          <div className="flex items-center gap-2">
            {[1, 1.25, 1.5, 2].map((value) => (
              <button
                key={value}
                type="button"
                onClick={() => setSpeed(value)}
                className={`h-8 rounded-btn border px-3 text-meta font-semibold ${
                  speed === value
                    ? "border-ink-2 bg-elevate text-ink"
                    : "border-line-strong text-ink hover:bg-elevate"
                }`}
              >
                {playbackRateLabel(value)}
              </button>
            ))}
          </div>
          <div className="break-all text-meta text-ink-3">
            {playbackStatus ? `${playbackStatus} / ` : null}
            {session.audioPath}
            <br />
            {audioSrc}
          </div>
        </div>
      ) : (
        <div className="rounded-card border border-warn bg-warn-soft px-4 py-3 text-body text-ink">
          音声ファイルがありません
        </div>
      )}
    </section>
  );
}

function TranscriptView({
  segments,
  currentMs,
  error,
  liveStatus,
  onSeek,
}: {
  segments: Segment[];
  currentMs: number;
  error: string | null;
  liveStatus: Extract<SessionStatus, "recording" | "transcribing"> | null;
  onSeek: (timeMs: number) => void;
}) {
  const [copyStatus, setCopyStatus] = useState<string | null>(null);
  const endRef = useRef<HTMLDivElement | null>(null);
  const hasSegments = segments.length > 0;
  const isLive = liveStatus !== null;

  useEffect(() => {
    if (!isLive || segments.length === 0) {
      return;
    }
    endRef.current?.scrollIntoView({ block: "end" });
  }, [isLive, segments.length]);

  const copyAll = async () => {
    const text = segments.map((segment) => segment.text).join("\n");
    try {
      await navigator.clipboard.writeText(text);
      setCopyStatus("コピーしました");
    } catch {
      setCopyStatus("コピーできませんでした");
    }
  };

  return (
    <section className="grid gap-3 border-t border-line pt-5">
      <div className="flex items-center justify-between gap-3">
        <div>
          <h2 className="text-title">文字起こし</h2>
          <div className="mt-1 flex flex-wrap items-center gap-2 text-meta text-ink-2">
            <span>{segments.length} セグメント</span>
            {liveStatus ? <LiveTranscriptBadge status={liveStatus} /> : null}
          </div>
        </div>
        <button
          type="button"
          onClick={() => void copyAll()}
          disabled={!hasSegments}
          className="h-9 rounded-btn border border-line-strong px-3 text-meta font-semibold text-ink hover:bg-elevate disabled:text-ink-3"
        >
          全文コピー
        </button>
      </div>

      {error ? (
        <div className="rounded-card border border-warn bg-warn-soft px-4 py-3 text-body text-ink">
          {error}
        </div>
      ) : null}

      {copyStatus ? <div className="text-meta text-ink-2">{copyStatus}</div> : null}

      {hasSegments ? (
        <div className="grid max-h-[52vh] gap-2 overflow-y-auto pr-1">
          {segments.map((segment) => {
            const active = currentMs >= segment.startMs && currentMs < segment.endMs;
            return (
              <button
                key={segment.id}
                type="button"
                onClick={() => onSeek(segment.startMs)}
                className={`grid grid-cols-[72px_1fr] gap-3 rounded-card border px-3 py-3 text-left ${
                  active
                    ? "border-accent bg-accent-soft"
                    : "border-line bg-surface hover:bg-surface-2"
                }`}
              >
                <span className="text-meta font-semibold text-ink-2">
                  {formatTimestamp(segment.startMs)}
                </span>
                <span className="text-body text-ink">{segment.text}</span>
              </button>
            );
          })}
          {isLive ? <PendingTranscriptRow status={liveStatus} /> : null}
          <div ref={endRef} />
        </div>
      ) : (
        <>
          {isLive ? (
            <PendingTranscriptRow status={liveStatus} />
          ) : (
            <div className="rounded-card border border-line bg-surface px-4 py-6 text-body text-ink-2">
              文字起こしはまだありません
            </div>
          )}
        </>
      )}
    </section>
  );
}

function LiveTranscriptBadge({
  status,
}: {
  status: Extract<SessionStatus, "recording" | "transcribing">;
}) {
  return (
    <span className="inline-flex h-6 items-center gap-2 rounded-btn border border-accent bg-accent-soft px-2 text-meta font-semibold text-accent">
      <span className="h-2 w-2 rounded-full bg-accent motion-safe:animate-rec-pulse" />
      {status === "recording" ? "ライブ録音中" : "文字起こし中"}
    </span>
  );
}

function PendingTranscriptRow({
  status,
}: {
  status: Extract<SessionStatus, "recording" | "transcribing">;
}) {
  return (
    <div className="grid grid-cols-[72px_1fr] gap-3 rounded-card border border-dashed border-line-strong bg-surface-2 px-3 py-3">
      <span className="text-meta font-semibold text-ink-3">LIVE</span>
      <span className="inline-flex items-center gap-2 text-body text-ink-2">
        <span className="h-2 w-2 rounded-full bg-accent motion-safe:animate-rec-pulse" />
        {status === "recording" ? "発話を待機中" : "次のセグメントを処理中"}
      </span>
    </div>
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

function formatTimestamp(timestampMs: number) {
  const totalSeconds = Math.floor(timestampMs / 1_000);
  const minutes = Math.floor(totalSeconds / 60);
  const seconds = totalSeconds % 60;
  return `${String(minutes).padStart(2, "0")}:${String(seconds).padStart(2, "0")}`;
}

function playbackRateLabel(value: number) {
  return `${value.toFixed(2).replace(/\.?0+$/, "")}x`;
}

function formatExtension(format: ExportFormat) {
  return format;
}

function formatLabel(format: ExportFormat) {
  const labels: Record<ExportFormat, string> = {
    txt: "Text",
    srt: "SubRip",
    md: "Markdown",
  };
  return labels[format];
}

function safeFileBase(title: string) {
  const safe = title.trim().replace(/[<>:"/\\|?*\x00-\x1f]/g, "_");
  return safe.length > 0 ? safe : "sokki-transcript";
}

function mergeSegment(current: Segment[], segment: Segment) {
  const withoutDuplicate = current.filter((item) => item.id !== segment.id);
  return [...withoutDuplicate, segment].sort((a, b) => a.startMs - b.startMs || a.id - b.id);
}

function errorMessage(error: unknown) {
  if (error && typeof error === "object" && "message" in error) {
    return String(error.message);
  }
  return String(error);
}
