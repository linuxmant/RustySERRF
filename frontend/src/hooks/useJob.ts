"use client";

import { useCallback, useEffect, useRef, useState } from "react";
import { flushSync } from "react-dom";
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
  const activeJobIdRef = useRef<string | null>(null);

  useEffect(() => {
    return () => {
      unsubscribeRef.current?.();
    };
  }, []);

  const submit = useCallback((file: File) => {
    setState({ phase: "uploading" });
    uploadDataset(file)
      .then(({ jobId }) => {
        activeJobIdRef.current = jobId;
        setState({ phase: "processing", jobId });
        unsubscribeRef.current = subscribeToJobEvents(jobId, (event: JobEvent) => {
          if (event.status === "progress") {
            // flushSync forces this render to commit immediately instead of being batched
            // with whatever other progress events the EventSource happens to deliver in
            // the same browser task — without it, React 18's automatic batching can
            // silently collapse a burst of rapid updates into just the last one, making
            // the UI appear to "jump" straight to a late stage instead of animating through
            // each one (see useJob.test.ts's "synchronous burst" regression test).
            flushSync(() => {
              setState({ phase: "processing", jobId, stage: event.stage, current: event.current, total: event.total });
            });
          } else if (event.status === "completed") {
            unsubscribeRef.current?.();
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
            setState({ phase: "error", message: event.error });
          }
        });
      })
      .catch((error: Error) => setState({ phase: "error", message: error.message }));
  }, []);

  const reset = useCallback(() => {
    unsubscribeRef.current?.();
    unsubscribeRef.current = null;
    activeJobIdRef.current = null;
    setState({ phase: "idle" });
  }, []);

  return { state, submit, reset };
}
