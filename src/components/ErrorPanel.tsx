import { TriangleAlert, X } from "lucide-react";
import type { JobError } from "../types";

interface Props {
  errors: JobError[];
  onDismiss: () => void;
}

/**
 * Failures of the current batch. The status line in the footer holds one message
 * at a time, so without this every failure but the last scrolled past unseen.
 */
export function ErrorPanel({ errors, onDismiss }: Props) {
  if (errors.length === 0) return null;

  return (
    <section className="error-panel" role="alert" aria-label="Failed entries">
      <div className="error-panel-head">
        <TriangleAlert size={16} aria-hidden />
        <strong>
          {errors.length} {errors.length === 1 ? "entry" : "entries"} failed
        </strong>
        <button
          type="button"
          className="icon-btn"
          title="Dismiss"
          aria-label="Dismiss error list"
          onClick={onDismiss}
        >
          <X size={16} aria-hidden />
        </button>
      </div>
      <ul className="error-panel-list">
        {errors.map((err) => (
          <li key={err.id}>
            <span className="error-panel-name">{err.displayName}</span>
            <span className="error-panel-msg mono">{err.message}</span>
          </li>
        ))}
      </ul>
    </section>
  );
}
