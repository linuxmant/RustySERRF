import Box from "@mui/material/Box";
import { BarChart, type BarItem } from "@mui/x-charts/BarChart";

interface RsdBarChartProps {
  compoundLabels: string[];
  qcRsdRaw: (number | null)[];
  qcRsdSerrf: (number | null)[];
}

const PIXELS_PER_COMPOUND = 24;
const MIN_CHART_WIDTH = 600;

function formatPercentLabel(item: BarItem): string | null {
  return typeof item.value === "number" ? `${(item.value * 100).toFixed(1)}%` : null;
}

export default function RsdBarChart({ compoundLabels, qcRsdRaw, qcRsdSerrf }: RsdBarChartProps) {
  const width = Math.max(MIN_CHART_WIDTH, compoundLabels.length * PIXELS_PER_COMPOUND);

  return (
    <Box sx={{ overflowX: "auto" }}>
      <BarChart
        width={width}
        height={420}
        xAxis={[
          {
            scaleType: "band",
            data: compoundLabels,
            label: "Compound",
            categoryGapRatio: 0.75,
            barGapRatio: 0.4,
            tickLabelStyle: { fontSize: 13, textAnchor: "middle" },
            labelStyle: { fontSize: 14, textAnchor: "middle" },
          },
        ]}
        yAxis={[{ label: "QC-RSD", tickLabelStyle: { fontSize: 13 }, labelStyle: { fontSize: 14, textAnchor: "middle" } }]}
        series={[
          { data: qcRsdRaw, label: "Raw QC-RSD", barLabel: formatPercentLabel, barLabelPlacement: "outside" },
          { data: qcRsdSerrf, label: "SERRF QC-RSD", barLabel: formatPercentLabel, barLabelPlacement: "outside" },
        ]}
      />
    </Box>
  );
}
