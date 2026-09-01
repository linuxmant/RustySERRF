"use client";

import { useContext } from "react";
import IconButton from "@mui/material/IconButton";
import Brightness4Icon from "@mui/icons-material/Brightness4";
import Brightness7Icon from "@mui/icons-material/Brightness7";
import { ColorModeContext } from "../app/ThemeRegistry";

export default function ThemeToggle() {
  const { mode, toggle } = useContext(ColorModeContext);

  return (
    <IconButton aria-label="toggle theme" aria-pressed={mode === "dark"} onClick={toggle}>
      {mode === "dark" ? <Brightness7Icon /> : <Brightness4Icon />}
    </IconButton>
  );
}
