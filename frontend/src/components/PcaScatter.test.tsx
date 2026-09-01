import { describe, expect, it } from "vitest";
import { render, screen } from "@testing-library/react";
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
});
