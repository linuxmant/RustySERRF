import { describe, expect, it } from "vitest";
import { render, screen } from "@testing-library/react";
import ProgressView from "./ProgressView";

describe("ProgressView", () => {
  it("shows an indeterminate bar and a default message before any progress event", () => {
    render(<ProgressView />);

    expect(screen.getByText(/starting normalization/i)).toBeInTheDocument();
    expect(screen.getByRole("progressbar")).not.toHaveAttribute("aria-valuenow");
  });

  it("shows a determinate bar and stage/current/total once progress arrives", () => {
    render(<ProgressView stage="SERRF normalization" current={3} total={10} />);

    expect(screen.getByText("SERRF normalization")).toBeInTheDocument();
    expect(screen.getByText("3 / 10")).toBeInTheDocument();
    expect(screen.getByRole("progressbar")).toHaveAttribute("aria-valuenow", "30");
  });
});
