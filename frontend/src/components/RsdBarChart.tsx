import Box from "@mui/material/Box";
import { BarChart } from "@mui/x-charts/BarChart";

interface RsdBarChartProps {
  compoundLabels: string[];
  qcRsdRaw: (number | null)[];
  qcRsdSerrf: (number | null)[];
}

const PIXELS_PER_COMPOUND = 24;
const MIN_CHART_WIDTH = 600;

export default function RsdBarChart({ compoundLabels, qcRsdRaw, qcRsdSerrf }: RsdBarChartProps) {
  const width = Math.max(MIN_CHART_WIDTH, compoundLabels.length * PIXELS_PER_COMPOUND);

  return (
    <Box sx={{ overflowX: "auto" }}>
      <BarChart
        width={width}
        height={400}
        xAxis={[{ scaleType: "band", data: compoundLabels, label: "Compound" }]}
        yAxis={[{ label: "QC-RSD" }]}
        series={[
          { data: qcRsdRaw, label: "Raw QC-RSD" },
          { data: qcRsdSerrf, label: "SERRF QC-RSD" },
        ]}
      />
    </Box>
  );
}
