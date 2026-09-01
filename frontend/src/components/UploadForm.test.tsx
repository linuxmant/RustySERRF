import { describe, expect, it, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import UploadForm from "./UploadForm";

describe("UploadForm", () => {
  it("disables submit until a file is chosen, then calls onSubmit with it", async () => {
    const onSubmit = vi.fn();
    render(<UploadForm onSubmit={onSubmit} />);

    const button = screen.getByRole("button", { name: /run serrf normalization/i });
    expect(button).toBeDisabled();

    const file = new File(["a,b\n1,2"], "dataset.csv", { type: "text/csv" });
    await userEvent.upload(screen.getByLabelText(/dataset file/i), file);
    expect(button).toBeEnabled();

    await userEvent.click(button);

    expect(onSubmit).toHaveBeenCalledWith(file);
  });

  it("shows an error message when provided", () => {
    render(<UploadForm onSubmit={vi.fn()} errorMessage="batch B has too few QC" />);

    expect(screen.getByText("batch B has too few QC")).toBeInTheDocument();
  });
});
