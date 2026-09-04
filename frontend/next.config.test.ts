import { describe, expect, it } from "vitest";
// @ts-expect-error next.config.js is plain JS and has no type declarations (allowJs is off)
import nextConfig from "./next.config.js";

describe("next.config.js", () => {
  it("disables response compression so the dev proxy doesn't buffer SSE progress events", () => {
    expect(nextConfig.compress).toBe(false);
  });
});
