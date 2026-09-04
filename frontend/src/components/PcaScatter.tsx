import Box from "@mui/material/Box";
import Typography from "@mui/material/Typography";
import { ScatterChart } from "@mui/x-charts/ScatterChart";

interface PcaScatterProps {
  pc1: number[];
  pc2: number[];
  sampleType: (string | null)[];
  batch?: string[];
  title: string;
}

export default function PcaScatter({ pc1, pc2, sampleType, batch, title }: PcaScatterProps) {
  const showBatch = batch !== undefined && new Set(batch).size > 1;

  const groupLabel = (index: number): string => {
    const type = sampleType[index] ?? "unknown";
    return showBatch && batch ? `${type} (${batch[index]})` : type;
  };

  const groups = Array.from(new Set(sampleType.map((_, index) => groupLabel(index))));
  const series = groups.map((group) => {
    const indices = sampleType
      .map((_, index) => (groupLabel(index) === group ? index : -1))
      .filter((index) => index !== -1);
    return {
      label: group,
      data: indices.map((index) => ({ id: index, x: pc1[index], y: pc2[index] })),
    };
  });

  return (
    <Box>
      <Typography variant="subtitle1" gutterBottom>
        {title}
      </Typography>
      <ScatterChart
        height={400}
        series={series}
        xAxis={[{ label: "PC1" }]}
        yAxis={[{ label: "PC2" }]}
        slotProps={{ legend: { direction: "horizontal", position: { vertical: "bottom", horizontal: "center" } } }}
      />
    </Box>
  );
}
