import type { Metadata } from "next";
import ThemeRegistry from "./ThemeRegistry";
import "./globals.css";

export const metadata: Metadata = {
  title: "RustySERRF",
  description: "SERRF normalization for metabolomics data",
};

// Known limitation, accepted as the cost of a flash-free first paint: this app is a
// static export, so the prerendered HTML always bakes in the light theme's Emotion
// class hashes (they're derived from the palette's literal hex values). A returning
// dark-mode visitor's first CLIENT render uses different classes than the server HTML,
// which is a real hydration mismatch — React recovers by re-rendering from the client
// markup, so this surfaces only as a console warning, not a visible bug. Fully removing
// it would mean moving to a CSS-variables-based MUI theme (`experimental_extendTheme`/
// `CssVarsProvider`), which emits one theme-independent class set and switches palettes
// via CSS custom properties instead — a larger migration, out of scope here.
//
// Runs before hydration to stamp document.documentElement with the persisted (or OS-preferred)
// color mode and paint matching MUI-default colors immediately, so ThemeRegistry's first client
// render — which reads the same attribute via getInitialMode() — never has to correct a
// wrongly-guessed initial theme after the fact.
const COLOR_MODE_INIT_SCRIPT = `(function () {
  try {
    var stored = localStorage.getItem("color-mode");
    var mode = stored === "light" || stored === "dark"
      ? stored
      : (window.matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light");
    document.documentElement.dataset.colorMode = mode;
    document.documentElement.style.colorScheme = mode;
    document.documentElement.style.backgroundColor = mode === "dark" ? "#121212" : "#fff";
    document.documentElement.style.color = mode === "dark" ? "#fff" : "rgba(0, 0, 0, 0.87)";
  } catch (e) {}
})();`;

export default function RootLayout({ children }: { children: React.ReactNode }) {
  return (
    <html lang="en" suppressHydrationWarning>
      <head>
        <script dangerouslySetInnerHTML={{ __html: COLOR_MODE_INIT_SCRIPT }} />
      </head>
      <body>
        <ThemeRegistry>{children}</ThemeRegistry>
      </body>
    </html>
  );
}
