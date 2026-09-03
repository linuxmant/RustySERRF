"use client";

import { createContext, useMemo, useState } from "react";
import CssBaseline from "@mui/material/CssBaseline";
import { ThemeProvider } from "@mui/material/styles";
import { AppRouterCacheProvider } from "@mui/material-nextjs/v14-appRouter";
import { getTheme } from "./theme";

export type ColorMode = "light" | "dark";

export const ColorModeContext = createContext<{ mode: ColorMode; toggle: () => void }>({
  mode: "light",
  toggle: () => {},
});

function getInitialMode(): ColorMode {
  // The blocking inline script in layout.tsx's <head> (COLOR_MODE_INIT_SCRIPT) runs before
  // hydration and stamps this attribute, so reading it here — rather than always starting at
  // "light" and correcting in a useEffect after mount — avoids a flash of the wrong theme.
  if (typeof document === "undefined") {
    return "light";
  }
  return document.documentElement.dataset.colorMode === "dark" ? "dark" : "light";
}

export default function ThemeRegistry({ children }: { children: React.ReactNode }) {
  const [mode, setMode] = useState<ColorMode>(getInitialMode);

  const contextValue = useMemo(
    () => ({
      mode,
      toggle: () => {
        setMode((current) => {
          const next = current === "light" ? "dark" : "light";
          localStorage.setItem("color-mode", next);
          document.documentElement.dataset.colorMode = next;
          return next;
        });
      },
    }),
    [mode]
  );

  const theme = useMemo(() => getTheme(mode), [mode]);

  return (
    <AppRouterCacheProvider>
      <ColorModeContext.Provider value={contextValue}>
        <ThemeProvider theme={theme}>
          <CssBaseline />
          {children}
        </ThemeProvider>
      </ColorModeContext.Provider>
    </AppRouterCacheProvider>
  );
}
