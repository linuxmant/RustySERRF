import Box from "@mui/material/Box";

// The one signature flourish on the page: a chromatogram/spectrum trace line, the single
// most characteristic image in this app's own domain (LC-MS metabolomics). Draws itself in
// on load via a CSS stroke-dashoffset animation; respects prefers-reduced-motion.
export default function ChromatogramDivider() {
  return (
    <Box
      component="svg"
      viewBox="0 0 400 40"
      aria-hidden="true"
      sx={{
        display: "block",
        width: "100%",
        maxWidth: 480,
        height: 32,
        color: "primary.main",
        "& path": {
          fill: "none",
          stroke: "currentColor",
          strokeWidth: 2,
          strokeLinecap: "round",
          strokeDasharray: 620,
          strokeDashoffset: 620,
          animation: "trace-draw 1.4s ease-out 0.15s forwards",
        },
        "@media (prefers-reduced-motion: reduce)": {
          "& path": { animation: "none", strokeDashoffset: 0 },
        },
        "@keyframes trace-draw": {
          to: { strokeDashoffset: 0 },
        },
      }}
    >
      <path d="M0,30 L40,30 C55,30 58,4 68,4 C78,4 82,30 96,30 L155,30 C170,30 173,14 181,14 C189,14 192,30 208,30 L280,30 C292,30 295,20 301,20 C307,20 310,30 320,30 L400,30" />
    </Box>
  );
}
