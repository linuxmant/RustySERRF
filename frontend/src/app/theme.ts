import { createTheme, type Theme } from "@mui/material/styles";

// Palette grounded in analytical chemistry: a chromatogram/spectrum trace, not a generic
// marketing palette. Paper is a cool clinical off-white (not the cream+serif cliché); the
// primary accent is the muted teal of a chromatography trace line; the secondary accent is
// used sparingly for contrast (e.g. before/after chart series, hover states).
const light = {
  paper: "#F7F9F9",
  surface: "#FFFFFF",
  ink: "#152521",
  border: "#DDE3E1",
  teal: "#2E7D6B",
  amber: "#C9622B",
};

const dark = {
  paper: "#0F1614",
  surface: "#182220",
  ink: "#E8EDEB",
  border: "#28332F",
  teal: "#4FB89E",
  amber: "#E08A4F",
};

export function getTheme(mode: "light" | "dark"): Theme {
  const tokens = mode === "light" ? light : dark;

  return createTheme({
    palette: {
      mode,
      background: { default: tokens.paper, paper: tokens.surface },
      text: { primary: tokens.ink },
      primary: { main: tokens.teal },
      secondary: { main: tokens.amber },
      divider: tokens.border,
    },
    shape: { borderRadius: 10 },
    typography: {
      fontFamily: "var(--font-plex-sans), Helvetica, Arial, sans-serif",
      h1: { fontWeight: 600, letterSpacing: "-0.01em" },
      h4: { fontWeight: 600, letterSpacing: "-0.01em" },
      h5: { fontWeight: 600 },
      button: { textTransform: "none", fontWeight: 600 },
    },
    components: {
      MuiPaper: {
        styleOverrides: {
          root: { backgroundImage: "none" },
        },
      },
      MuiButton: {
        styleOverrides: {
          root: { borderRadius: 8 },
        },
      },
    },
  });
}

export const dataFontFamily = "var(--font-plex-mono), ui-monospace, SFMono-Regular, monospace";
