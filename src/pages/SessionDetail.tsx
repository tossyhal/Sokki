import { useEffect, useRef, useState } from "react";
import { convertFileSrc } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useNavigate, useParams } from "react-router-dom";
import { getSegments } from "../lib/api";
import { formatDuration } from "../lib/format";
import type { Segment, Session, SessionStatus, Source } from "../lib/types";
import { useSessionStore } from "../stores/useSessionStore";

export default function SessionDetail() {
  const { id } = useParams();
  const navigate = useNavigate();
  const { selected, loading, cancelingId, error, loadOne, cancel } = useSessionStore();
  const audioRef = useRef<HTMLAudioElement | null>(null);
  const [segments, setSegments] = useState<Segment[]>([]);
  const [segmentError, setSegmentError] = useState<string | null>(null);
  const [currentMs, setCurrentMs] = useState(0);

  useEffect(() => {
    if (id) {
      void loadOne(id);
    }
  }, [id, loadOne]);

  const session = selected?.id === id ? selected : null;

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
        {session?.status === "transcribing" ? (
          <button
            type="button"
            onClick={() => void cancel(session.id)}
            disabled={cancelingId === session.id}
            className="h-10 shrink-0 rounded-btn border border-line-strong px-4 text-body font-semibold text-ink hover:bg-elevate disabled:text-ink-3"
          >
            {cancelingId === session.id ? "停止中" : "文字起こし停止"}
          </button>
        ) : null}
      </div>

      {error ? (
        <div className="rounded-card border border-warn bg-warn-soft px-4 py-3 text-body text-ink">
          {error}
        </div>
      ) : null}

      {loading && !session ? (
        <div className="border-t border-line py-8 text-body text-ink-2">読み込み中</div>
      ) : null}

      {session ? (
        <>
          {session.status === "interrupted" ? (
            <div className="rounded-card border border-warn bg-warn-soft px-4 py-3 text-body text-ink">
              処理が中断されました。再文字起こしできます。
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
            transcribing={session.status === "transcribing"}
            onSeek={(timeMs) => {
              if (audioRef.current) {
                audioRef.current.currentTime = timeMs / 1_000;
              }
            }}
          />
        </>
      ) : null}
    </section>
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
  transcribing,
  onSeek,
}: {
  segments: Segment[];
  currentMs: number;
  error: string | null;
  transcribing: boolean;
  onSeek: (timeMs: number) => void;
}) {
  const [copyStatus, setCopyStatus] = useState<string | null>(null);
  const hasSegments = segments.length > 0;

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
          <p className="mt-1 text-meta text-ink-2">
            {segments.length} セグメント{transcribing ? " / 処理中" : ""}
          </p>
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
        <div className="grid gap-2">
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
        </div>
      ) : (
        <div className="rounded-card border border-line bg-surface px-4 py-6 text-body text-ink-2">
          {transcribing ? "文字起こし中" : "文字起こしはまだありません"}
        </div>
      )}
    </section>
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
