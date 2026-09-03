import { defineConfig } from "eslint/config";
import nextCoreWebVitals from "eslint-config-next/core-web-vitals";
import nextTypescript from "eslint-config-next/typescript";

export default defineConfig([
  {
    ignores: ["coverage/**", ".next/**", "out/**", "playwright-report/**", "test-results/**"],
  },
  {
    extends: [...nextCoreWebVitals, ...nextTypescript],
  },
]);