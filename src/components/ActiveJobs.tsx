import { Languages, Loader2, Rss, Sparkles, Users } from "lucide-react";
import { useT } from "../i18n/I18nProvider";
import { detailsForRow, jobPercent } from "../lib/jobs";
import type { JobRow } from "../types";

interface Props {
  jobs: JobRow[];
}

const STAGE_ICON = {
  download: Rss,
  whisper: Languages,
  diarize: Users,
  llm: Sparkles,
} as const;

/**
 * The entries the backend is working on, lifted out of the table.
 *
 * There are up to **two**: the pipeline runs Whisper and the LLM concurrently
 * through a channel of capacity one, so while file *n* is summarised, file
 * *n+1* is already being transcribed. In the plain table both sat among the
 * waiting rows with no more visual weight than them, and that two things were
 * happening at once was not visible at all.
 *
 * Only `download` and `whisper` report a percentage; the other stages show an
 * indeterminate marker rather than a fabricated number.
 */
export function ActiveJobs({ jobs }: Props) {
  const { t } = useT();
  if (jobs.length === 0) return null;

  return (
    <section className="active-jobs" aria-label={t("active.title")}>
      {jobs.map((job) => {
        const Icon = STAGE_ICON[job.stage as keyof typeof STAGE_ICON] ?? Languages;
        const pct = jobPercent(job);
        return (
          <div className="active-job" key={job.path}>
            <Icon size={18} aria-hidden />
            <span className="active-job-text">
              <span className="active-job-name mono">{job.displayName}</span>
              <span className="active-job-stage">{detailsForRow(job, t)}</span>
            </span>
            {pct == null ? (
              <Loader2 size={15} className="icon spin active-job-spinner" aria-hidden />
            ) : (
              <div
                className="progress-track active-job-progress"
                role="progressbar"
                aria-valuemin={0}
                aria-valuemax={100}
                aria-valuenow={pct}
                aria-label={job.displayName}
              >
                <div className="progress-bar" style={{ width: `${Math.min(100, pct)}%` }} />
              </div>
            )}
          </div>
        );
      })}
    </section>
  );
}
