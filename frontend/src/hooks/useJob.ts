"use client";

import { useCallback, useRef, useState } from "react";
import { fetchJobResult, subscribeToJobEvents, uploadDataset } from "../lib/api";
import type { JobEvent, ResultJson } from "../lib/types";

export type JobState =
  | { phase: "idle" }
  | { phase: "uploading" }
  | { phase: "processing"; jobId: string; stage?: string; current?: number; total?: number }
  | { phase: "done"; jobId: string; result: ResultJson }
  | { phase: "error"; message: string };

export function useJob() {
  const [state, setState] = useState<JobState>({ phase: "idle" });
  const unsubscribeRef = useRef<(() => void) | null>(null);

  const submit = useCallback((file: File) => {
    setState({ phase: "uploading" });
    uploadDataset(file)
      .then(({ jobId }) => {
        setState({ phase: "processing", jobId });
        unsubscribeRef.current = subscribeToJobEvents(jobId, (event: JobEvent) => {
          if (event.status === "progress") {
            setState({ phase: "processing", jobId, stage: event.stage, current: event.current, total: event.total });
          } else if (event.status === "completed") {
            unsubscribeRef.current?.();
            fetchJobResult(jobId)
              .then((result) => setState({ phase: "done", jobId, result }))
              .catch((error: Error) => setState({ phase: "error", message: error.message }));
          } else if (event.status === "failed") {
            unsubscribeRef.current?.();
            setState({ phase: "error", message: event.error });
          }
        });
      })
      .catch((error: Error) => setState({ phase: "error", message: error.message }));
  }, []);

  const reset = useCallback(() => {
    unsubscribeRef.current?.();
    unsubscribeRef.current = null;
    setState({ phase: "idle" });
  }, []);

  return { state, submit, reset };
}
