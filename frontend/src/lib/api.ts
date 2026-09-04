import type { JobEvent, ResultJson } from "./types";

function apiBase(): string {
  return process.env.NEXT_PUBLIC_API_BASE ?? "";
}

export class ApiError extends Error {
  status: number;

  constructor(message: string, status: number) {
    super(message);
    this.status = status;
  }
}

async function parseErrorMessage(response: Response): Promise<string> {
  if (response.status === 413) {
    return "File is too large (max 10MB).";
  }
  try {
    const body = (await response.json()) as { error?: string };
    return typeof body.error === "string" ? body.error : response.statusText;
  } catch {
    return response.statusText;
  }
}

export async function uploadDataset(file: File): Promise<{ jobId: string }> {
  const form = new FormData();
  form.append("file", file);
  const response = await fetch(`${apiBase()}/api/jobs`, { method: "POST", body: form });
  if (!response.ok) {
    throw new ApiError(await parseErrorMessage(response), response.status);
  }
  const body = (await response.json()) as { job_id: string };
  return { jobId: body.job_id };
}

export function subscribeToJobEvents(jobId: string, onEvent: (event: JobEvent) => void): () => void {
  const source = new EventSource(`${apiBase()}/api/jobs/${jobId}/events`);
  const statuses: JobEvent["status"][] = ["queued", "progress", "completed", "failed"];
  const registered = statuses.map((status) => {
    const listener = (message: MessageEvent<string>) => {
      onEvent(JSON.parse(message.data) as JobEvent);
    };
    source.addEventListener(status, listener as EventListener);
    return { status, listener };
  });

  source.onerror = () => {
    if (source.readyState !== EventSource.CLOSED) {
      return;
    }
    fetchJobStatus(jobId)
      .then((status) => {
        if (status.status === "completed" || status.status === "failed") {
          onEvent(status);
        } else {
          onEvent({ status: "failed", error: "Lost connection to the server while the job was still running." });
        }
      })
      .catch(() => {
        onEvent({ status: "failed", error: "Lost connection to the server and could not confirm job status." });
      });
  };

  return () => {
    registered.forEach(({ status, listener }) => source.removeEventListener(status, listener as EventListener));
    source.close();
  };
}

export async function fetchJobStatus(jobId: string): Promise<JobEvent> {
  const response = await fetch(`${apiBase()}/api/jobs/${jobId}`);
  if (!response.ok) {
    throw new ApiError(await parseErrorMessage(response), response.status);
  }
  return (await response.json()) as JobEvent;
}

export async function fetchJobResult(jobId: string): Promise<ResultJson> {
  const response = await fetch(`${apiBase()}/api/jobs/${jobId}/result`);
  if (!response.ok) {
    throw new ApiError(await parseErrorMessage(response), response.status);
  }
  return (await response.json()) as ResultJson;
}

export function downloadUrl(jobId: string): string {
  return `${apiBase()}/api/jobs/${jobId}/download`;
}
