import { describe, expect, it } from "vitest";
import { render, screen } from "@testing-library/react";
import Box from "@mui/material/Box";
import ThemeRegistry from "./ThemeRegistry";

describe("ThemeRegistry", () => {
  it("renders children through the app's theme", () => {
    render(
      <ThemeRegistry>
        <button>hello</button>
      </ThemeRegistry>
    );

    expect(screen.getByRole("button", { name: "hello" })).toBeInTheDocument();
  });

  it("applies the app's teal primary color, not the MUI default blue", () => {
    render(
      <ThemeRegistry>
        <Box data-testid="probe" sx={{ color: "primary.main" }} />
      </ThemeRegistry>
    );

    expect(getComputedStyle(screen.getByTestId("probe")).color).toBe("rgb(46, 125, 107)");
  });
});
