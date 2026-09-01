import { describe, expect, it } from "vitest";
import { render, screen, within } from "@testing-library/react";
import PcaScatter from "./PcaScatter";

describe("PcaScatter", () => {
  it("renders a titled chart with one legend entry per sample type", () => {
    const { container } = render(
      <PcaScatter
        title="Before normalization"
        pc1={[1, 2, 3]}
        pc2={[4, 5, 6]}
        sampleType={["qc", "sample", null]}
      />
    );

    expect(screen.getByText("Before normalization")).toBeInTheDocument();
    expect(container.querySelector("svg")).toBeInTheDocument();
    expect(screen.getByText("qc")).toBeInTheDocument();
    expect(screen.getByText("sample")).toBeInTheDocument();
    expect(screen.getByText("unknown")).toBeInTheDocument();
  });

  it("qualifies the legend with batch when multiple batches are present", () => {
    const { container } = render(
      <PcaScatter
        title="Before normalization"
        pc1={[1, 2, 3]}
        pc2={[4, 5, 6]}
        sampleType={["qc", "qc", "sample"]}
        batch={["A", "A", "B"]}
      />
    );

    // Scoped to the rendered container: MUI X Charts leaves a stray
    // off-screen measurement span attached directly to document.body (outside
    // this component's tree) that can transiently echo the last-measured
    // label text, which would otherwise cause screen-level queries to match
    // more than one element.
    expect(within(container).getByText("qc (A)")).toBeInTheDocument();
    expect(within(container).getByText("sample (B)")).toBeInTheDocument();
  });
});
