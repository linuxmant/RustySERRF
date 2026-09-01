import Box from "@mui/material/Box";
import Button from "@mui/material/Button";
import Grid from "@mui/material/Grid";
import Typography from "@mui/material/Typography";
import { downloadUrl } from "../lib/api";
import type { ResultJson } from "../lib/types";
import RsdBarChart from "./RsdBarChart";
import PcaScatter from "./PcaScatter";

interface ResultsViewProps {
  jobId: string;
  result: ResultJson;
  onReset: () => void;
}

function median(values: (number | null)[]): number {
  const finite = values.filter((value): value is number => typeof value === "number" && Number.isFinite(value));
  const sorted = [...finite].sort((a, b) => a - b);
  return sorted.length === 0 ? 0 : sorted[Math.floor(sorted.length / 2)];
}

export default function ResultsView({ jobId, result, onReset }: ResultsViewProps) {
  return (
    <Box>
      <Typography variant="h5" gutterBottom>
        Results
      </Typography>
      <Typography variant="body1" sx={{ mb: 2 }}>
        {result.compound_labels.length} compounds — median QC-RSD raw {median(result.qc_rsd_raw).toFixed(3)}, SERRF{" "}
        {median(result.qc_rsd_serrf).toFixed(3)}
      </Typography>
      <Grid container spacing={4}>
        <Grid item xs={12}>
          <RsdBarChart
            compoundLabels={result.compound_labels}
            qcRsdRaw={result.qc_rsd_raw}
            qcRsdSerrf={result.qc_rsd_serrf}
          />
        </Grid>
        <Grid item xs={12} md={6}>
          <PcaScatter
            title="Before normalization"
            pc1={result.pca_before.pc1}
            pc2={result.pca_before.pc2}
            sampleType={result.pca_before.sample_type}
            batch={result.pca_before.batch}
          />
        </Grid>
        <Grid item xs={12} md={6}>
          <PcaScatter
            title="After normalization"
            pc1={result.pca_after.pc1}
            pc2={result.pca_after.pc2}
            sampleType={result.pca_after.sample_type}
            batch={result.pca_after.batch}
          />
        </Grid>
      </Grid>
      <Box sx={{ mt: 3, display: "flex", gap: 2 }}>
        <Button variant="contained" href={downloadUrl(jobId)}>
          Download results (.zip)
        </Button>
        <Button variant="outlined" onClick={onReset}>
          Start a new run
        </Button>
      </Box>
    </Box>
  );
}
