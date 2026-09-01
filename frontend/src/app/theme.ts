import { createTheme, type Theme } from "@mui/material/styles";

export function getTheme(mode: "light" | "dark"): Theme {
  return createTheme({ palette: { mode } });
}
