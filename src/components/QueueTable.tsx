import { FileText, FolderOpen } from "lucide-react";
import { useEffect, useRef } from "react";
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
      <p className="empty-title">
        Add audio files (or drop them anywhere in this window) or podcast episodes, then press
        Start.
      </p>
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
                aria-label="Select all entries"
              />
            </th>
            <th scope="col" style={{ width: "40%" }}>
              File / Episode
            </th>
            <th scope="col" style={{ width: "90px" }}>
              Status
            </th>
            <th scope="col">Details</th>
            <th scope="col" style={{ width: 72 }}>
              <span className="visually-hidden">Output</span>
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
            const badge = badgeForStage(job.stage);
            const outputPath = outputPathOf(job);
            return (
              <tr key={item.id}>
                <td>
                  <input
                    type="checkbox"
                    checked={selected.has(item.id)}
                    disabled={processing}
                    onChange={() => onToggle(item.id)}
                    aria-label={`Select ${item.displayName}`}
                  />
                </td>
                <td className="mono">{item.displayName}</td>
                <td>
                  <span className={`badge ${badge.className}`}>{badge.label}</span>
                </td>
                <td className="mono details-cell">{detailsForRow(job)}</td>
                <td className="row-actions">
                  {outputPath ? (
                    <>
                      <button
                        type="button"
                        className="icon-btn"
                        title={`Open ${outputPath}`}
                        aria-label={`Open the Markdown file for ${item.displayName}`}
                        onClick={() => onOpenResult(outputPath)}
                      >
                        <FileText size={16} aria-hidden />
                      </button>
                      <button
                        type="button"
                        className="icon-btn"
                        title="Show in file manager"
                        aria-label={`Show the output folder for ${item.displayName}`}
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
