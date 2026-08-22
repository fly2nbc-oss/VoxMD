import { FileText, FolderOpen } from "lucide-react";
import { useEffect, useRef } from "react";
import { useT } from "../i18n/I18nProvider";
import { badgeForStage, detailsForRow, outputPathOf } from "../lib/jobs";
import type { JobRow, QueueItem } from "../types";

interface Props {
  items: QueueItem[];
  jobs: Record<string, JobRow>;
  selected: Set<string>;
  processing: boolean;
  onToggle: (id: string) => void;
  onToggleAll: (checked: boolean) => void;
  onOpenResult: (path: string) => void;
  onRevealResult: (path: string) => void;
}

export function QueueTable({
  items,
  jobs,
  selected,
  processing,
  onToggle,
  onToggleAll,
  onOpenResult,
  onRevealResult,
}: Props) {
  const { t } = useT();
  const headerCheckbox = useRef<HTMLInputElement | null>(null);
  const allSelected = items.length > 0 && selected.size === items.length;

  // `indeterminate` has no HTML attribute; it can only be set on the element.
  useEffect(() => {
    if (headerCheckbox.current) {
      headerCheckbox.current.indeterminate = selected.size > 0 && selected.size < items.length;
    }
  }, [selected, items]);

  if (items.length === 0) {
    return (
      <p className="empty-title">{t("queue.empty")}</p>
    );
  }

  return (
    <div className="table-wrap">
      <table className="table">
        <thead>
          <tr>
            <th scope="col" style={{ width: 34 }}>
              <input
                ref={headerCheckbox}
                type="checkbox"
                checked={allSelected}
                disabled={processing}
                onChange={(e) => onToggleAll(e.target.checked)}
                aria-label={t("queue.selectAll")}
              />
            </th>
            <th scope="col" style={{ width: "40%" }}>
              {t("queue.colFile")}
            </th>
            <th scope="col" style={{ width: "90px" }}>
              {t("queue.colStatus")}
            </th>
            <th scope="col">{t("queue.colDetails")}</th>
            <th scope="col" style={{ width: 72 }}>
              <span className="visually-hidden">{t("queue.colOutput")}</span>
            </th>
          </tr>
        </thead>
        <tbody>
          {items.map((item) => {
            const job = jobs[item.id] ?? {
              path: item.id,
              displayName: item.displayName,
              stage: "queued",
            };
            const badge = badgeForStage(job.stage, t);
            const outputPath = outputPathOf(job);
            return (
              <tr key={item.id}>
                <td>
                  <input
                    type="checkbox"
                    checked={selected.has(item.id)}
                    disabled={processing}
                    onChange={() => onToggle(item.id)}
                    aria-label={t("queue.select", { name: item.displayName })}
                  />
                </td>
                <td className="mono">{item.displayName}</td>
                <td>
                  <span className={`badge ${badge.className}`}>{badge.label}</span>
                </td>
                <td className="mono details-cell">{detailsForRow(job, t)}</td>
                <td className="row-actions">
                  {outputPath ? (
                    <>
                      <button
                        type="button"
                        className="icon-btn"
                        title={t("queue.open", { path: outputPath })}
                        aria-label={t("queue.openAria", { name: item.displayName })}
                        onClick={() => onOpenResult(outputPath)}
                      >
                        <FileText size={16} aria-hidden />
                      </button>
                      <button
                        type="button"
                        className="icon-btn"
                        title={t("queue.reveal")}
                        aria-label={t("queue.revealAria", { name: item.displayName })}
                        onClick={() => onRevealResult(outputPath)}
                      >
                        <FolderOpen size={16} aria-hidden />
                      </button>
                    </>
                  ) : null}
                </td>
              </tr>
            );
          })}
        </tbody>
      </table>
    </div>
  );
}
