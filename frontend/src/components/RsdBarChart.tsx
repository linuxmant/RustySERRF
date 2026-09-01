import { BarChart } from "@mui/x-charts/BarChart";

interface RsdBarChartProps {
  compoundLabels: string[];
  qcRsdRaw: number[];
  qcRsdSerrf: number[];
}

export default function RsdBarChart({ compoundLabels, qcRsdRaw, qcRsdSerrf }: RsdBarChartProps) {
  return (
    <BarChart
      height={400}
      xAxis={[{ scaleType: "band", data: compoundLabels, label: "Compound" }]}
      yAxis={[{ label: "QC-RSD" }]}
      series={[
        { data: qcRsdRaw, label: "Raw QC-RSD" },
        { data: qcRsdSerrf, label: "SERRF QC-RSD" },
      ]}
    />
  );
}
