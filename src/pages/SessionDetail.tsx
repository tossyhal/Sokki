import { useEffect, useState } from "react";
import { convertFileSrc } from "@tauri-apps/api/core";
import { useNavigate, useParams } from "react-router-dom";
import { formatDuration } from "../lib/format";
import type { Session, SessionStatus, Source } from "../lib/types";
import { useSessionStore } from "../stores/useSessionStore";

export default function SessionDetail() {
  const { id } = useParams();
  const navigate = useNavigate();
  const { selected, loading, error, loadOne } = useSessionStore();

  useEffect(() => {
    if (id) {
      void loadOne(id);
    }
  }, [id, loadOne]);

  const session = selected?.id === id ? selected : null;

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
          <AudioPlayer session={session} />
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

function AudioPlayer({ session }: { session: Session }) {
  const audioSrc = session.audioPath ? convertFileSrc(session.audioPath) : null;
  const [playbackStatus, setPlaybackStatus] = useState<string | null>(null);

  useEffect(() => {
    setPlaybackStatus(null);
  }, [session.audioPath]);

  return (
    <section className="grid gap-3 border-t border-line pt-5">
      <div>
        <h2 className="text-title">音声</h2>
        <p className="mt-1 text-meta text-ink-2">録音WAV</p>
      </div>

      {audioSrc ? (
        <div className="grid gap-2">
          <audio
            key={audioSrc}
            controls
            src={audioSrc}
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
