import { describe, expect, it, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import Home from "./page";
import { useJob } from "../hooks/useJob";
import type { JobState } from "../hooks/useJob";

vi.mock("../lib/api");
vi.mock("../hooks/useJob");

const resultFixture = {
  compound_labels: ["c1"],
  qc_rsd_raw: [0.1],
  qc_rsd_serrf: [0.01],
  validate_rsd_raw: {},
  validate_rsd_serrf: {},
  pca_before: { pc1: [1], pc2: [2], sample_type: ["qc"], batch: ["A"] },
  pca_after: { pc1: [1], pc2: [2], sample_type: ["qc"], batch: ["A"] },
};

function mockJobState(state: JobState) {
  vi.mocked(useJob).mockReturnValue({ state, submit: vi.fn(), reset: vi.fn() });
}

describe("Home", () => {
  it("renders the app title and starts in the upload state", () => {
    mockJobState({ phase: "idle" });
    render(<Home />);

    expect(screen.getByRole("heading", { name: "RustySERRF" })).toBeInTheDocument();
    expect(screen.getByText(/upload a dataset/i)).toBeInTheDocument();
  });

  it("styles the heading through the theme instead of an unstyled raw <h1>", () => {
    mockJobState({ phase: "idle" });
    render(<Home />);

    const heading = screen.getByRole("heading", { name: "RustySERRF" });
    expect(heading.tagName).toBe("H1");
    expect(heading.className).toMatch(/Mui/);
  });

  it("shows an indeterminate progress view while uploading", () => {
    mockJobState({ phase: "uploading" });
    render(<Home />);

    expect(screen.getByText(/starting normalization/i)).toBeInTheDocument();
  });

  it("shows the processing stage/current/total while a job runs", () => {
    mockJobState({ phase: "processing", jobId: "job-1", stage: "SERRF normalization", current: 3, total: 10 });
    render(<Home />);

    expect(screen.getByText("SERRF normalization")).toBeInTheDocument();
    expect(screen.getByText("3 / 10")).toBeInTheDocument();
  });

  it("shows results when the job is done", () => {
    mockJobState({ phase: "done", jobId: "job-1", result: resultFixture });
    render(<Home />);

    expect(screen.getByRole("heading", { name: "Results" })).toBeInTheDocument();
  });

  it("shows an error message and a reset button when the job fails", () => {
    const resetMock = vi.fn();
    vi.mocked(useJob).mockReturnValue({
      state: { phase: "error", message: "batch B has too few QC" },
      submit: vi.fn(),
      reset: resetMock,
    });
    render(<Home />);

    expect(screen.getByText("batch B has too few QC")).toBeInTheDocument();
    screen.getByRole("button", { name: /start over/i }).click();
    expect(resetMock).toHaveBeenCalled();
  });
});
