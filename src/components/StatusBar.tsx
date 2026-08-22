import { Loader2 } from "lucide-react";
import { useT } from "../i18n/I18nProvider";

interface Props {
  itemCount: number;
  overall: { completed: number; total: number } | null;
  modelDownload: { pct: number; model: string } | null;
  processing: boolean;
  cancelling: boolean;
  statusMsg: string;
}

export function StatusBar({
  itemCount,
  overall,
  modelDownload,
  processing,
  cancelling,
  statusMsg,
}: Props) {
  const { t } = useT();
  const overallPct = overall && overall.total > 0 ? (overall.completed / overall.total) * 100 : 0;
  const pct = modelDownload ? modelDownload.pct : overallPct;

  const summary = modelDownload
    ? t("status.downloading", { model: modelDownload.model })
    : overall
      ? t("status.overall", { done: overall.completed, total: overall.total })
      : itemCount
        ? t("status.queuedCount", { count: itemCount })
        : t("status.empty");

  return (
    <footer className="meta-bar">
      <span>{summary}</span>
      <div
        className="progress-track"
        role="progressbar"
        aria-valuemin={0}
        aria-valuemax={100}
        aria-valuenow={Math.round(pct)}
        aria-label={modelDownload ? t("status.modelProgress") : t("status.batchProgress")}
        title={
          modelDownload
            ? t("status.modelProgressTitle", { pct: modelDownload.pct })
            : t("status.overallTitle")
        }
      >
        <div className="progress-bar" style={{ width: `${Math.min(100, pct)}%` }} />
      </div>
      {/* Announced to screen readers: this line is the only feedback channel for
          stage changes, results and errors. */}
      <span className="status-slot" aria-live="polite" aria-atomic="true">
        {modelDownload ? (
          <span className="mono">{`${modelDownload.pct}%`}</span>
        ) : processing ? (
          <>
            <Loader2 size={14} className="icon spin" aria-hidden />
            <span>{cancelling ? t("status.cancelling") : t("status.running")}</span>
          </>
        ) : (
          <span className="mono status-text">{statusMsg}</span>
        )}
      </span>
    </footer>
  );
}
