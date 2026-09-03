import { afterEach, describe, expect, it } from "vitest";
import { render, screen } from "@testing-library/react";
import { useContext } from "react";
import ThemeRegistry, { ColorModeContext } from "./ThemeRegistry";

function ModeProbe() {
  const { mode } = useContext(ColorModeContext);
  return <div data-testid="mode">{mode}</div>;
}

describe("ThemeRegistry", () => {
  afterEach(() => {
    delete document.documentElement.dataset.colorMode;
  });

  it("picks up the color mode already stamped on <html> by the pre-hydration script, without waiting for an effect", () => {
    document.documentElement.dataset.colorMode = "dark";

    render(
      <ThemeRegistry>
        <ModeProbe />
      </ThemeRegistry>
    );

    expect(screen.getByTestId("mode")).toHaveTextContent("dark");
  });

  it("defaults to light when no color mode has been stamped on <html>", () => {
    render(
      <ThemeRegistry>
        <ModeProbe />
      </ThemeRegistry>
    );

    expect(screen.getByTestId("mode")).toHaveTextContent("light");
  });
});
