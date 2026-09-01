import { describe, expect, it, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import ResultsView from "./ResultsView";
import type { ResultJson } from "../lib/types";

const result: ResultJson = {
  compound_labels: ["c1", "c2"],
  qc_rsd_raw: [0.2, 0.4],
  qc_rsd_serrf: [0.02, 0.04],
  validate_rsd_raw: {},
  validate_rsd_serrf: {},
  pca_before: { pc1: [1, 2], pc2: [3, 4], sample_type: ["qc", "sample"], batch: ["A", "A"] },
  pca_after: { pc1: [1, 2], pc2: [3, 4], sample_type: ["qc", "sample"], batch: ["A", "A"] },
};

describe("ResultsView", () => {
  it("shows a summary, both PCA panels, and a working download link and reset button", async () => {
    const onReset = vi.fn();
    render(<ResultsView jobId="job-1" result={result} onReset={onReset} />);

    expect(screen.getByText(/2 compounds/i)).toBeInTheDocument();
    expect(screen.getByText("Before normalization")).toBeInTheDocument();
    expect(screen.getByText("After normalization")).toBeInTheDocument();

    const downloadLink = screen.getByRole("link", { name: /download results/i });
    expect(downloadLink).toHaveAttribute("href", "/api/jobs/job-1/download");

    await userEvent.click(screen.getByRole("button", { name: /start a new run/i }));
    expect(onReset).toHaveBeenCalled();
  });
});
