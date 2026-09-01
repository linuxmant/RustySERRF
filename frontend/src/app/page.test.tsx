import { describe, expect, it, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import Home from "./page";

vi.mock("../lib/api");

describe("Home", () => {
  it("renders the app title and starts in the upload state", () => {
    render(<Home />);

    expect(screen.getByRole("heading", { name: "RustySERRF" })).toBeInTheDocument();
    expect(screen.getByText(/upload a dataset/i)).toBeInTheDocument();
  });
});
