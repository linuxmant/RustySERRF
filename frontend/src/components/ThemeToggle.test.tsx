import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import ThemeRegistry from "../app/ThemeRegistry";
import ThemeToggle from "./ThemeToggle";

describe("ThemeToggle", () => {
  beforeEach(() => {
    localStorage.clear();
  });

  afterEach(() => {
    delete document.documentElement.dataset.colorMode;
    vi.unstubAllGlobals();
  });

  it("defaults to light mode and persists a switch to dark on click", async () => {
    render(
      <ThemeRegistry>
        <ThemeToggle />
      </ThemeRegistry>
    );
    const button = screen.getByRole("button", { name: /toggle theme/i });
    expect(localStorage.getItem("color-mode")).toBeNull();

    await userEvent.click(button);

    expect(localStorage.getItem("color-mode")).toBe("dark");
  });

  it("initializes from a previously persisted mode", () => {
    document.documentElement.dataset.colorMode = "dark";
    render(
      <ThemeRegistry>
        <ThemeToggle />
      </ThemeRegistry>
    );
    expect(screen.getByRole("button", { name: /toggle theme/i })).toHaveAttribute("aria-pressed", "true");
  });
});
