import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useEffect, useState } from "react";
import type {
  BatchCompletePayload,
  JobError,
  JobProgressPayload,
  JobRow,
  ModelDownloadPayload,
} from "../types";

export interface BatchState {
  jobs: Record<string, JobRow>;
  setJobs: React.Dispatch<React.SetStateAction<Record<string, JobRow>>>;
  processing: boolean;
  setProcessing: React.Dispatch<React.SetStateAction<boolean>>;
  cancelling: boolean;
  setCancelling: React.Dispatch<React.SetStateAction<boolean>>;
  overall: { completed: number; total: number } | null;
  setOverall: React.Dispatch<React.SetStateAction<{ completed: number; total: number } | null>>;
  statusMsg: string;
  setStatusMsg: React.Dispatch<React.SetStateAction<string>>;
  errors: JobError[];
  setErrors: React.Dispatch<React.SetStateAction<JobError[]>>;
  modelDownload: { pct: number; model: string } | null;
}

/** Subscribes to the backend's progress events for the lifetime of the app. */
export function useBatchEvents(): BatchState {
  const [jobs, setJobs] = useState<Record<string, JobRow>>({});
  const [processing, setProcessing] = useState(false);
  const [cancelling, setCancelling] = useState(false);
  const [overall, setOverall] = useState<{ completed: number; total: number } | null>(null);
  const [statusMsg, setStatusMsg] = useState("");
  const [errors, setErrors] = useState<JobError[]>([]);
  const [modelDownload, setModelDownload] = useState<{ pct: number; model: string } | null>(null);

  // The backend owns the running state; recover it so a reload cannot leave the
  // UI disabled behind a batch that already finished.
  useEffect(() => {
    void invoke<boolean>("processing_state")
      .then(setProcessing)
      .catch(() => {});
  }, []);

  useEffect(() => {
    // `await listen(...)` can resolve after unmount, so handles are collected
    // and any that arrive late are stopped immediately.
    const unlisteners: Array<() => void> = [];
    let cancelled = false;
    const track = (stop: () => void) => {
      if (cancelled) stop();
      else unlisteners.push(stop);
    };

    (async () => {
      track(
        await listen<JobProgressPayload>("job_progress", (e) => {
          const p = e.payload;
          // Batch-wide notices (e.g. GPU fallback) use an empty path and must
          // not create a phantom queue row.
          if (p.path) {
            setJobs((prev) => ({
              ...prev,
              [p.path]: {
                ...p,
                downloadPct: p.downloadPct ?? prev[p.path]?.downloadPct,
                whisperPct: p.whisperPct ?? prev[p.path]?.whisperPct,
              },
            }));
          }
          if (p.overall) {
            setOverall({ completed: p.overall.completed, total: p.overall.total });
          }
          if (p.stage === "error" && p.path) {
            setErrors((prev) => [
              ...prev.filter((x) => x.id !== p.path),
              { id: p.path, displayName: p.displayName, message: p.message ?? "Failed." },
            ]);
          }
          if (
            p.message &&
            (p.stage === "done" ||
              p.stage === "error" ||
              p.stage === "skipped" ||
              !p.path)
          ) {
            setStatusMsg(p.message);
          }
        }),
      );

      track(
        await listen<BatchCompletePayload>("batch_complete", (e) => {
          setProcessing(false);
          setCancelling(false);
          setModelDownload(null);
          if (e.payload.error) setStatusMsg(e.payload.error);
          else if (e.payload.cancelled) setStatusMsg("Batch cancelled.");
          else setStatusMsg("Batch complete.");
        }),
      );

      track(
        await listen<ModelDownloadPayload>("model_download_progress", (e) => {
          const { stage, model, pct } = e.payload;
          if (stage === "ready" || stage === "resolving") {
            setModelDownload(null);
          } else if (stage === "downloading" && model != null && pct != null) {
            setModelDownload({ pct, model });
          }
        }),
      );
    })();

    return () => {
      cancelled = true;
      for (const stop of unlisteners) stop();
    };
  }, []);

  return {
    jobs,
    setJobs,
    processing,
    setProcessing,
    cancelling,
    setCancelling,
    overall,
    setOverall,
    statusMsg,
    setStatusMsg,
    errors,
    setErrors,
    modelDownload,
  };
}
