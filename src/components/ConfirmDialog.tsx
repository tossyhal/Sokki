export function ConfirmDialog({
  title,
  message,
  confirmLabel,
  confirming,
  onCancel,
  onConfirm,
}: {
  title: string;
  message: string;
  confirmLabel: string;
  confirming?: boolean;
  onCancel: () => void;
  onConfirm: () => void;
}) {
  return (
    <div className="fixed inset-0 z-30 flex items-center justify-center bg-black/25 px-4">
      <section className="w-full max-w-md rounded-card border border-line bg-surface p-5 shadow-panel">
        <h2 className="text-title text-ink">{title}</h2>
        <p className="mt-3 whitespace-pre-wrap text-body text-ink-2">{message}</p>
        <div className="mt-5 flex justify-end gap-2">
          <button
            type="button"
            onClick={onCancel}
            disabled={confirming}
            className="h-10 rounded-btn border border-line-strong px-4 text-body font-semibold text-ink hover:bg-elevate disabled:text-ink-3"
          >
            キャンセル
          </button>
          <button
            type="button"
            onClick={onConfirm}
            disabled={confirming}
            className="h-10 rounded-btn bg-accent px-4 text-body font-semibold text-white shadow-accent hover:bg-accent-hover disabled:bg-ink-3 disabled:shadow-none"
          >
            {confirming ? "処理中" : confirmLabel}
          </button>
        </div>
      </section>
    </div>
  );
}
