import { defineConfig } from "@playwright/test";

export default defineConfig({
  testDir: "./e2e",
  timeout: 120_000,
  use: { baseURL: "http://localhost:3000" },
  webServer: [
    {
      command: "cargo run --release -p serrf-api",
      cwd: "..",
      port: 8080,
      timeout: 300_000,
      reuseExistingServer: !process.env.CI,
    },
    {
      command: "npm run build && npm run dev",
      port: 3000,
      timeout: 180_000,
      reuseExistingServer: !process.env.CI,
    },
  ],
});
