import { act, renderHook, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { useJob } from "./useJob";
import * as api from "../lib/api";
import type { JobEvent, ResultJson } from "../lib/types";

vi.mock("../lib/api");

const resultFixture: ResultJson = {
  compound_labels: ["c1"],
  qc_rsd_raw: [0.1],
  qc_rsd_serrf: [0.01],
  validate_rsd_raw: {},
  validate_rsd_serrf: {},
  pca_before: { pc1: [1], pc2: [2], sample_type: ["qc"], batch: ["A"] },
  pca_after: { pc1: [1], pc2: [2], sample_type: ["qc"], batch: ["A"] },
};

afterEach(() => {
  vi.resetAllMocks();
});

describe("useJob", () => {
  it("moves idle -> processing -> done as events arrive", async () => {
    let emit: (event: JobEvent) => void = () => {};
    vi.mocked(api.uploadDataset).mockResolvedValue({ jobId: "job-1" });
    vi.mocked(api.subscribeToJobEvents).mockImplementation((_jobId, onEvent) => {
      emit = onEvent;
      return () => {};
    });
    vi.mocked(api.fetchJobResult).mockResolvedValue(resultFixture);

    const { result } = renderHook(() => useJob());
    expect(result.current.state).toEqual({ phase: "idle" });

    await act(async () => {
      result.current.submit(new File(["x"], "dataset.csv"));
    });
    await waitFor(() => expect(result.current.state.phase).toBe("processing"));

    act(() => emit({ status: "progress", stage: "SERRF normalization", current: 3, total: 10 }));
    expect(result.current.state).toEqual({
      phase: "processing",
      jobId: "job-1",
      stage: "SERRF normalization",
      current: 3,
      total: 10,
    });

    await act(async () => emit({ status: "completed" }));
    await waitFor(() =>
      expect(result.current.state).toEqual({ phase: "done", jobId: "job-1", result: resultFixture })
    );
  });

  it("moves to error when fetching the result fails after the job completes", async () => {
    let emit: (event: JobEvent) => void = () => {};
    vi.mocked(api.uploadDataset).mockResolvedValue({ jobId: "job-1" });
    vi.mocked(api.subscribeToJobEvents).mockImplementation((_jobId, onEvent) => {
      emit = onEvent;
      return () => {};
    });
    vi.mocked(api.fetchJobResult).mockRejectedValue(new Error("result fetch failed"));

    const { result } = renderHook(() => useJob());
    await act(async () => result.current.submit(new File(["x"], "dataset.csv")));
    await waitFor(() => expect(result.current.state.phase).toBe("processing"));

    await act(async () => emit({ status: "completed" }));

    await waitFor(() =>
      expect(result.current.state).toEqual({ phase: "error", message: "result fetch failed" })
    );
  });

  it("moves to error when the job fails", async () => {
    let emit: (event: JobEvent) => void = () => {};
    vi.mocked(api.uploadDataset).mockResolvedValue({ jobId: "job-1" });
    vi.mocked(api.subscribeToJobEvents).mockImplementation((_jobId, onEvent) => {
      emit = onEvent;
      return () => {};
    });

    const { result } = renderHook(() => useJob());
    await act(async () => result.current.submit(new File(["x"], "dataset.csv")));
    await waitFor(() => expect(result.current.state.phase).toBe("processing"));

    act(() => emit({ status: "failed", error: "batch B has too few QC" }));

    expect(result.current.state).toEqual({ phase: "error", message: "batch B has too few QC" });
  });

  it("moves to error when the upload itself rejects", async () => {
    vi.mocked(api.uploadDataset).mockRejectedValue(new Error("network down"));

    const { result } = renderHook(() => useJob());
    await act(async () => result.current.submit(new File(["x"], "dataset.csv")));

    await waitFor(() => expect(result.current.state).toEqual({ phase: "error", message: "network down" }));
  });

  it("reset returns to idle from any state", async () => {
    vi.mocked(api.uploadDataset).mockRejectedValue(new Error("boom"));
    const { result } = renderHook(() => useJob());
    await act(async () => result.current.submit(new File(["x"], "dataset.csv")));
    await waitFor(() => expect(result.current.state.phase).toBe("error"));

    act(() => result.current.reset());

    expect(result.current.state).toEqual({ phase: "idle" });
  });

  it("does not clobber idle state when reset() is called before a pending fetchJobResult resolves", async () => {
    let emit: (event: JobEvent) => void = () => {};
    let resolveFetch: (result: ResultJson) => void = () => {};
    vi.mocked(api.uploadDataset).mockResolvedValue({ jobId: "job-1" });
    vi.mocked(api.subscribeToJobEvents).mockImplementation((_jobId, onEvent) => {
      emit = onEvent;
      return () => {};
    });
    vi.mocked(api.fetchJobResult).mockImplementation(
      () =>
        new Promise<ResultJson>((resolve) => {
          resolveFetch = resolve;
        })
    );

    const { result } = renderHook(() => useJob());
    await act(async () => result.current.submit(new File(["x"], "dataset.csv")));
    await waitFor(() => expect(result.current.state.phase).toBe("processing"));

    // "completed" kicks off fetchJobResult, which we deliberately leave pending.
    await act(async () => emit({ status: "completed" }));

    // User navigates away / starts over before the fetch resolves.
    act(() => result.current.reset());
    expect(result.current.state).toEqual({ phase: "idle" });

    // The orphaned fetch now resolves — it must NOT clobber the reset state.
    await act(async () => {
      resolveFetch(resultFixture);
    });

    expect(result.current.state).toEqual({ phase: "idle" });
  });
});
