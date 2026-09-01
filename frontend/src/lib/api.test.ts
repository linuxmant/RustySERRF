import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { ApiError, downloadUrl, fetchJobResult, fetchJobStatus, subscribeToJobEvents, uploadDataset } from "./api";

class FakeEventSource {
  static instances: FakeEventSource[] = [];
  listeners: Record<string, ((event: MessageEvent) => void)[]> = {};
  closed = false;

  constructor(public url: string) {
    FakeEventSource.instances.push(this);
  }

  addEventListener(type: string, listener: (event: MessageEvent) => void) {
    this.listeners[type] = [...(this.listeners[type] ?? []), listener];
  }

  removeEventListener(type: string, listener: (event: MessageEvent) => void) {
    this.listeners[type] = (this.listeners[type] ?? []).filter((l) => l !== listener);
  }

  emit(type: string, data: unknown) {
    for (const listener of this.listeners[type] ?? []) {
      listener({ data: JSON.stringify(data) } as MessageEvent);
    }
  }

  close() {
    this.closed = true;
  }
}

beforeEach(() => {
  FakeEventSource.instances = [];
  vi.stubGlobal("EventSource", FakeEventSource);
});

afterEach(() => {
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

describe("uploadDataset", () => {
  it("posts multipart form data and returns the job id", async () => {
    const fetchMock = vi.fn().mockResolvedValue({
      ok: true,
      json: async () => ({ job_id: "abc-123" }),
    });
    vi.stubGlobal("fetch", fetchMock);

    const file = new File(["a,b\n1,2"], "dataset.csv", { type: "text/csv" });
    const result = await uploadDataset(file);

    expect(result).toEqual({ jobId: "abc-123" });
    expect(fetchMock).toHaveBeenCalledWith("/api/jobs", expect.objectContaining({ method: "POST" }));
  });

  it("throws ApiError with the server's message on a non-ok response", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn().mockResolvedValue({ ok: false, status: 400, json: async () => ({ error: "bad batch" }) })
    );

    const file = new File(["x"], "dataset.csv");
    await expect(uploadDataset(file)).rejects.toMatchObject(new ApiError("bad batch", 400));
  });
});

describe("subscribeToJobEvents", () => {
  it("parses progress events from the SSE stream", () => {
    const events: unknown[] = [];
    const unsubscribe = subscribeToJobEvents("job-1", (event) => events.push(event));

    const source = FakeEventSource.instances[0];
    expect(source.url).toBe("/api/jobs/job-1/events");
    source.emit("progress", { status: "progress", stage: "SERRF normalization", current: 1, total: 10 });

    expect(events).toEqual([{ status: "progress", stage: "SERRF normalization", current: 1, total: 10 }]);

    unsubscribe();
    expect(source.closed).toBe(true);
  });
});

describe("fetchJobStatus and fetchJobResult", () => {
  it("fetchJobStatus GETs the status endpoint", async () => {
    const fetchMock = vi.fn().mockResolvedValue({ ok: true, json: async () => ({ status: "queued" }) });
    vi.stubGlobal("fetch", fetchMock);

    const result = await fetchJobStatus("job-1");

    expect(fetchMock).toHaveBeenCalledWith("/api/jobs/job-1");
    expect(result).toEqual({ status: "queued" });
  });

  it("fetchJobResult GETs the result endpoint", async () => {
    const resultJson = {
      compound_labels: ["c1"],
      qc_rsd_raw: [0.1],
      qc_rsd_serrf: [0.01],
      validate_rsd_raw: {},
      validate_rsd_serrf: {},
      pca_before: { pc1: [1], pc2: [2], sample_type: ["qc"], batch: ["A"] },
      pca_after: { pc1: [1], pc2: [2], sample_type: ["qc"], batch: ["A"] },
    };
    vi.stubGlobal("fetch", vi.fn().mockResolvedValue({ ok: true, json: async () => resultJson }));

    const result = await fetchJobResult("job-1");

    expect(result).toEqual(resultJson);
  });
});

describe("downloadUrl", () => {
  it("builds the download path for a job id", () => {
    expect(downloadUrl("job-1")).toBe("/api/jobs/job-1/download");
  });
});
