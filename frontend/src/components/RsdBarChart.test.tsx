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

  it("labels each bar with its RSD value as a percentage, not a raw fraction", () => {
    render(<RsdBarChart compoundLabels={["c1"]} qcRsdRaw={[0.234]} qcRsdSerrf={[0.05]} />);

    expect(screen.getByText("23.4%")).toBeInTheDocument();
    expect(screen.getByText("5.0%")).toBeInTheDocument();
  });

  it("gives the chart enough width to stay readable at real compound counts, inside a horizontally scrollable container", () => {
    const compoundLabels = Array.from({ length: 268 }, (_, i) => `c${i}`);
    const values = compoundLabels.map(() => 0.1);
    const { container } = render(<RsdBarChart compoundLabels={compoundLabels} qcRsdRaw={values} qcRsdSerrf={values} />);

    // MUI X Charts renders the chart SVG with a viewBox attribute, not width/height attributes.
    // The viewBox format is "0 0 width height", so we extract the width from there.
    const chartSvg = container.querySelector("svg.MuiChartsSvgLayer-root");
    expect(chartSvg).toBeInTheDocument();
    const viewBox = chartSvg?.getAttribute("viewBox");
    const viewBoxWidth = viewBox ? Number(viewBox.split(" ")[2]) : 0;
    expect(viewBoxWidth).toBeGreaterThanOrEqual(268 * 20);

    // MUI Box applies the overflow style via CSS class, not inline style, so check computed style.
    const scrollContainer = container.firstElementChild as HTMLElement;
    const computedStyle = window.getComputedStyle(scrollContainer);
    expect(computedStyle.overflowX).toBe("auto");
  });
});
