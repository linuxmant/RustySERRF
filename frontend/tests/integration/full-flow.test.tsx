import { afterAll, afterEach, beforeAll, describe, expect, it, vi } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { spawn, type ChildProcessWithoutNullStreams } from "node:child_process";
import { readFileSync } from "node:fs";
import path from "node:path";
import Home from "../../src/app/page";

let apiProcess: ChildProcessWithoutNullStreams;

// --- jsdom/Node fetch interop shims -----------------------------------------
//
// The real `UploadForm` -> `useJob` -> `lib/api.ts` code path we exercise here
// uses jsdom's `FormData`/`File`/`Blob` and jsdom's/Node's `EventSource`. In a
// real browser all of these come from one implementation and interoperate
// fine. Inside vitest's jsdom test environment they don't:
//
//   1. jsdom's `FormData`/`File` cannot be serialized into a multipart body by
//      Node's native `fetch` (it throws "Invalid `boundary` for
//      `multipart/form-data` request"), and jsdom's `Blob` doesn't implement
//      `arrayBuffer()`/`stream()` for a third-party encoder to read bytes out
//      of it either -- only the legacy `FileReader` API works.
//   2. Node's real global `EventSource` (available via jsdom in newer Node, or
//      via `--experimental-eventsource`) extends `EventTarget`, and jsdom's
//      environment overrides the global `EventTarget`/`Event`/`MessageEvent`
//      classes for DOM purposes. Node's built-in EventSource ends up
//      dispatching events constructed against its own internal `Event`
//      binding against jsdom's `EventTarget.dispatchEvent`, which rejects them
//      ("The 'event' argument must be an instance of Event. Received an
//      instance of Event") -- a realm mismatch, not an app or server bug.
//
// Both shims below still perform genuine network I/O against the real
// `serrf-api` process spawned in `beforeAll` -- they only bridge the
// jsdom-vs-Node object-identity gap for encoding/decoding, exactly as a real
// browser's native implementations would. Nothing about the HTTP request/
// response bytes, the SSE stream, or the server's behavior is faked.
const realFetch = globalThis.fetch;

function readBlobAsArrayBuffer(blob: Blob): Promise<ArrayBuffer> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => resolve(reader.result as ArrayBuffer);
    reader.onerror = () => reject(reader.error as Error);
    reader.readAsArrayBuffer(blob);
  });
}

async function formDataToMultipart(form: FormData): Promise<{ body: Buffer; contentType: string }> {
  const boundary = `----vitestboundary${Math.random().toString(16).slice(2)}`;
  const chunks: Buffer[] = [];
  for (const [name, value] of form.entries()) {
    chunks.push(Buffer.from(`--${boundary}\r\n`));
    if (value instanceof File) {
      chunks.push(Buffer.from(`Content-Disposition: form-data; name="${name}"; filename="${value.name}"\r\n`));
      chunks.push(Buffer.from(`Content-Type: ${value.type || "application/octet-stream"}\r\n\r\n`));
      chunks.push(Buffer.from(await readBlobAsArrayBuffer(value)));
    } else {
      chunks.push(Buffer.from(`Content-Disposition: form-data; name="${name}"\r\n\r\n`));
      chunks.push(Buffer.from(String(value)));
    }
    chunks.push(Buffer.from("\r\n"));
  }
  chunks.push(Buffer.from(`--${boundary}--\r\n`));
  return { body: Buffer.concat(chunks), contentType: `multipart/form-data; boundary=${boundary}` };
}

async function jsdomInteropFetch(input: RequestInfo | URL, init?: RequestInit): Promise<Response> {
  if (init?.body instanceof FormData) {
    const { body, contentType } = await formDataToMultipart(init.body);
    return realFetch(input, {
      ...init,
      body: body as unknown as BodyInit,
      headers: { ...(init.headers ?? {}), "Content-Type": contentType },
    });
  }
  return realFetch(input, init);
}

type SseMessage = { data: string };
type SseListener = (message: SseMessage) => void;

/** Minimal real SSE client built on the real `fetch`, standing in for the
 * browser's native `EventSource` for the reasons explained above. It performs
 * a genuine streamed GET request against the real server and parses the
 * standard `event: <name>\ndata: <json>\n\n` wire format. */
class FetchEventSource {
  private listeners = new Map<string, Set<SseListener>>();
  private closed = false;
  private reader: ReadableStreamDefaultReader<Uint8Array> | undefined;

  constructor(url: string) {
    void this.run(url);
  }

  private async run(url: string): Promise<void> {
    // Deliberately no AbortController here: jsdom's AbortController/AbortSignal
    // are jsdom's own classes, and Node's native fetch rejects a jsdom
    // AbortSignal with "Expected signal to be an instance of AbortSignal" for
    // the same realm-mismatch reason documented above. `close()` instead
    // cancels the stream reader directly, which is a plain method call with
    // no cross-realm identity check.
    const response = await realFetch(url, { headers: { Accept: "text/event-stream" } });
    const reader = response.body?.getReader();
    if (!reader) return;
    this.reader = reader;
    const decoder = new TextDecoder();
    let buffer = "";
    while (!this.closed) {
      const { done, value } = await reader.read();
      if (done) return;
      buffer += decoder.decode(value, { stream: true });
      let separatorIndex: number;
      while ((separatorIndex = buffer.indexOf("\n\n")) !== -1) {
        const rawEvent = buffer.slice(0, separatorIndex);
        buffer = buffer.slice(separatorIndex + 2);
        this.dispatch(rawEvent);
      }
    }
  }

  private dispatch(rawEvent: string): void {
    let eventName = "message";
    const dataLines: string[] = [];
    for (const line of rawEvent.split("\n")) {
      if (line.startsWith("event:")) eventName = line.slice(6).trim();
      else if (line.startsWith("data:")) dataLines.push(line.slice(5).trim());
    }
    const data = dataLines.join("\n");
    for (const listener of this.listeners.get(eventName) ?? []) {
      listener({ data });
    }
  }

  addEventListener(type: string, listener: SseListener): void {
    if (!this.listeners.has(type)) this.listeners.set(type, new Set());
    this.listeners.get(type)!.add(listener);
  }

  removeEventListener(type: string, listener: SseListener): void {
    this.listeners.get(type)?.delete(listener);
  }

  close(): void {
    this.closed = true;
    void this.reader?.cancel().catch(() => {});
  }
}
// ---------------------------------------------------------------------------

beforeAll(async () => {
  const apiBase = await new Promise<string>((resolve, reject) => {
    apiProcess = spawn("cargo", ["run", "-p", "serrf-api"], {
      cwd: path.resolve(__dirname, "../../.."),
      env: { ...process.env, PORT: "0" },
    });
    let settled = false;
    apiProcess.stdout.on("data", (chunk: Buffer) => {
      const match = chunk.toString().match(/listening on (\S+)/);
      if (match && !settled) {
        settled = true;
        resolve(`http://${match[1].replace("0.0.0.0", "127.0.0.1")}`);
      }
    });
    apiProcess.on("error", reject);
    apiProcess.on("exit", (code) => {
      if (!settled) reject(new Error(`serrf-api exited early with code ${code}`));
    });
  });
  process.env.NEXT_PUBLIC_API_BASE = apiBase;
}, 300_000);

afterAll(() => {
  apiProcess?.kill();
  delete process.env.NEXT_PUBLIC_API_BASE;
});

afterEach(() => {
  vi.unstubAllGlobals();
});

describe("full upload-to-results flow against a real serrf-api", () => {
  it("uploads a dataset, watches progress, and renders downloadable results", async () => {
    vi.stubGlobal("fetch", jsdomInteropFetch);
    vi.stubGlobal("EventSource", FetchEventSource);
    render(<Home />);

    const csvContent = readFileSync(path.resolve(__dirname, "../fixtures/example-dataset.csv"), "utf-8");
    const file = new File([csvContent], "dataset.csv", { type: "text/csv" });
    await userEvent.upload(screen.getByLabelText(/dataset file/i), file);
    await userEvent.click(screen.getByRole("button", { name: /run serrf normalization/i }));

    await waitFor(() => expect(screen.getByText("Results")).toBeInTheDocument(), { timeout: 60_000 });

    const downloadLink = screen.getByRole("link", { name: /download results/i });
    expect(downloadLink.getAttribute("href")).toMatch(/\/api\/jobs\/.+\/download/);
  }, 65_000);
});
