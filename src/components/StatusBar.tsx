import { Loader2 } from "lucide-react";
import { useT } from "../i18n/I18nProvider";
import type { QueueCounts } from "../lib/jobs";

interface Props {
  counts: QueueCounts;
  overall: { completed: number; total: number } | null;
  modelDownload: { pct: number; model: string } | null;
  processing: boolean;
  cancelling: boolean;
  statusMsg: string;
}

export function StatusBar({
  counts,
  overall,
  modelDownload,
  processing,
  cancelling,
  statusMsg,
}: Props) {
  const { t } = useT();
  const overallPct = overall && overall.total > 0 ? (overall.completed / overall.total) * 100 : 0;
  const pct = modelDownload ? modelDownload.pct : overallPct;

  // Plain counts. "Overall: 0 / 1 done (MD)" counted only the running batch, so
  // it read as a contradiction next to a queue holding ninety-nine entries —
  // and "(MD)" is not a word.
  const parts: string[] = [];
  if (counts.waiting > 0) parts.push(t("status.counts", { waiting: counts.waiting }));
  if (counts.done > 0) parts.push(t("status.countsDone", { done: counts.done }));
  if (counts.failed > 0) parts.push(t("status.countsFailed", { failed: counts.failed }));

  const summary = modelDownload
    ? t("status.downloading", { model: modelDownload.model })
    : parts.length > 0
      ? parts.join(t("status.countsSep"))
      : t("status.nothing");

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
