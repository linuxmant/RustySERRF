"use client";

import { useCallback, useEffect, useRef, useState } from "react";
import { fetchJobResult, subscribeToJobEvents, uploadDataset } from "../lib/api";
import type { JobEvent, ResultJson } from "../lib/types";

export type JobState =
  | { phase: "idle" }
  | { phase: "uploading" }
  | { phase: "processing"; jobId: string; stage?: string; current?: number; total?: number }
  | { phase: "done"; jobId: string; result: ResultJson }
  | { phase: "error"; message: string };

interface PendingProgress {
  jobId: string;
  stage?: string;
  current?: number;
  total?: number;
}

export function useJob() {
  const [state, setState] = useState<JobState>({ phase: "idle" });
  const unsubscribeRef = useRef<(() => void) | null>(null);
  const activeJobIdRef = useRef<string | null>(null);
  const pendingProgressRef = useRef<PendingProgress | null>(null);
  const rafIdRef = useRef<number | null>(null);

  const cancelScheduledRender = useCallback(() => {
    if (rafIdRef.current !== null) {
      cancelAnimationFrame(rafIdRef.current);
      rafIdRef.current = null;
    }
  }, []);

  useEffect(() => {
    return () => {
      unsubscribeRef.current?.();
      cancelScheduledRender();
    };
  }, [cancelScheduledRender]);

  const submit = useCallback(
    (file: File) => {
      setState({ phase: "uploading" });
      uploadDataset(file)
        .then(({ jobId }) => {
          activeJobIdRef.current = jobId;
          setState({ phase: "processing", jobId });
          unsubscribeRef.current = subscribeToJobEvents(jobId, (event: JobEvent) => {
            if (event.status === "progress") {
              // The browser can dispatch a whole burst of buffered SSE messages
              // synchronously in one JS task (plausible now that normalization is fast) —
              // no amount of forcing React to commit mid-task makes the browser actually
              // repaint before that task finishes, since painting only ever happens
              // between tasks. Decoupling rendering from event-arrival rate fixes this:
              // stash the latest event and let requestAnimationFrame apply it once per
              // real display frame, so progress visibly animates no matter how bursty
              // delivery is, and multiple events in one frame coalesce to the latest value
              // instead of wastefully re-rendering once per event.
              pendingProgressRef.current = { jobId, stage: event.stage, current: event.current, total: event.total };
              if (rafIdRef.current === null) {
                rafIdRef.current = requestAnimationFrame(() => {
                  rafIdRef.current = null;
                  const pending = pendingProgressRef.current;
                  if (!pending) {
                    return;
                  }
                  // Check the actual current state, not just a ref, so a frame that was
                  // already in flight when the job completed/failed/reset can never
                  // regress the UI back to "processing" with stale data.
                  setState((current) => {
                    if (current.phase !== "processing" || current.jobId !== pending.jobId) {
                      return current;
                    }
                    return { phase: "processing", ...pending };
                  });
                });
              }
            } else if (event.status === "completed") {
              unsubscribeRef.current?.();
              cancelScheduledRender();
              fetchJobResult(jobId)
                .then((result) => {
                  if (activeJobIdRef.current === jobId) {
                    setState({ phase: "done", jobId, result });
                  }
                })
                .catch((error: Error) => {
                  if (activeJobIdRef.current === jobId) {
                    setState({ phase: "error", message: error.message });
                  }
                });
            } else if (event.status === "failed") {
              unsubscribeRef.current?.();
              cancelScheduledRender();
              setState({ phase: "error", message: event.error });
            }
          });
        })
        .catch((error: Error) => setState({ phase: "error", message: error.message }));
    },
    [cancelScheduledRender]
  );

  const reset = useCallback(() => {
    unsubscribeRef.current?.();
    unsubscribeRef.current = null;
    activeJobIdRef.current = null;
    cancelScheduledRender();
    setState({ phase: "idle" });
  }, [cancelScheduledRender]);

  return { state, submit, reset };
}
