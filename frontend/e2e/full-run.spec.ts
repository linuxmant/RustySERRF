import { test, expect } from "@playwright/test";
import path from "node:path";

test("upload a dataset, watch progress, view results, download, and toggle theme", async ({ page }) => {
  await page.goto("/");

  await page
    .getByLabel("dataset file")
    .setInputFiles(path.resolve(__dirname, "../tests/fixtures/example-dataset.csv"));
  await page.getByRole("button", { name: "Run SERRF normalization" }).click();

  await expect(page.getByRole("heading", { name: "Results" })).toBeVisible({ timeout: 300_000 });
  await expect(page.locator("svg").first()).toBeVisible();

  const downloadPromise = page.waitForEvent("download");
  await page.getByRole("link", { name: /download results/i }).click();
  const download = await downloadPromise;
  expect(download.suggestedFilename()).toBe("serrf-results.zip");

  const toggle = page.getByRole("button", { name: /toggle theme/i });
  await expect(toggle).toHaveAttribute("aria-pressed", "false");
  await toggle.click();
  await expect(toggle).toHaveAttribute("aria-pressed", "true");
  const persisted = await page.evaluate(() => localStorage.getItem("color-mode"));
  expect(persisted).toBe("dark");
});
