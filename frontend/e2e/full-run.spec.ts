import { test, expect } from "@playwright/test";
import path from "node:path";

test("upload a dataset, watch progress, view results, and download", async ({ page }) => {
  await page.goto("/");

  await page
    .getByLabel("dataset file")
    .setInputFiles(path.resolve(__dirname, "../tests/fixtures/example-dataset.csv"));
  await page.getByRole("button", { name: "Run SERRF normalization" }).click();

  await expect(page.getByRole("heading", { name: "Results" })).toBeVisible({ timeout: 60_000 });
  await expect(page.locator("svg").first()).toBeVisible();

  const downloadPromise = page.waitForEvent("download");
  await page.getByRole("link", { name: /download results/i }).click();
  const download = await downloadPromise;
  expect(download.suggestedFilename()).toBe("serrf-results.zip");
});
