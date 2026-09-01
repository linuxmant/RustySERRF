import Box from "@mui/material/Box";
import Typography from "@mui/material/Typography";
import { ScatterChart } from "@mui/x-charts/ScatterChart";

interface PcaScatterProps {
  pc1: number[];
  pc2: number[];
  sampleType: (string | null)[];
  title: string;
}

export default function PcaScatter({ pc1, pc2, sampleType, title }: PcaScatterProps) {
  const groups = Array.from(new Set(sampleType.map((type) => type ?? "unknown")));
  const series = groups.map((group) => {
    const indices = sampleType
      .map((type, index) => ((type ?? "unknown") === group ? index : -1))
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
      <ScatterChart height={400} series={series} xAxis={[{ label: "PC1" }]} yAxis={[{ label: "PC2" }]} />
    </Box>
  );
}
