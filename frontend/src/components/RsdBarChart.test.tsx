import { describe, expect, it } from "vitest";
import { render, screen } from "@testing-library/react";
import RsdBarChart from "./RsdBarChart";

describe("RsdBarChart", () => {
  it("renders a chart with both series labeled", () => {
    const { container } = render(
      <RsdBarChart compoundLabels={["c1", "c2"]} qcRsdRaw={[0.2, 0.3]} qcRsdSerrf={[0.05, 0.06]} />
    );

    expect(container.querySelector("svg")).toBeInTheDocument();
    expect(screen.getByText("Raw QC-RSD")).toBeInTheDocument();
    expect(screen.getByText("SERRF QC-RSD")).toBeInTheDocument();
  });
});
