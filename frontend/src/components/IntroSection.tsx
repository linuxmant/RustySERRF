import Box from "@mui/material/Box";
import Link from "@mui/material/Link";
import Stack from "@mui/material/Stack";
import Typography from "@mui/material/Typography";

const STEPS = [
  { title: "Upload your dataset", detail: "A .csv or .xlsx file in the SERRF layout — batch, sample type, and time rows above your compound data." },
  { title: "Run the normalization", detail: "SERRF trains a random forest per compound and batch, then removes systematic drift." },
  { title: "Download your results", detail: "Normalized values, a before/after QC-RSD comparison, and a PCA report, all in one .zip." },
];

export default function IntroSection() {
  return (
    <Box component="section" sx={{ mb: 4 }}>
      <Stack spacing={1.5} sx={{ mb: 3 }}>
        {STEPS.map((step, index) => (
          <Box key={step.title} sx={{ display: "flex", gap: 2, alignItems: "baseline" }}>
            <Typography
              component="span"
              sx={{
                fontFamily: "var(--font-plex-mono), monospace",
                color: "primary.main",
                fontWeight: 600,
                minWidth: 20,
              }}
            >
              {index + 1}
            </Typography>
            <Typography component="span">
              <Typography component="span" sx={{ fontWeight: 600 }}>
                {step.title}
              </Typography>
              {" — "}
              {step.detail}
            </Typography>
          </Box>
        ))}
      </Stack>

      <Typography variant="body2" color="text.secondary" sx={{ mb: 0.5 }}>
        Leave a cell blank for any sample you don&rsquo;t want normalized — SERRF passes blanks through untouched.
      </Typography>
      <Typography variant="body2" color="text.secondary">
        Need sample data? <Link href="/example-dataset.xlsx">Use the example dataset</Link>.
      </Typography>
    </Box>
  );
}
