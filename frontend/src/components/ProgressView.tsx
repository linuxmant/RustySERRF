import Box from "@mui/material/Box";
import LinearProgress from "@mui/material/LinearProgress";
import Typography from "@mui/material/Typography";

interface ProgressViewProps {
  stage?: string;
  current?: number;
  total?: number;
}

export default function ProgressView({ stage, current, total }: ProgressViewProps) {
  const value = current !== undefined && total ? Math.min(100, (current / total) * 100) : undefined;

  return (
    <Box>
      <Typography variant="h6" gutterBottom>
        {stage ?? "Starting normalization…"}
      </Typography>
      {value === undefined ? <LinearProgress /> : <LinearProgress variant="determinate" value={value} />}
      {current !== undefined && total !== undefined && (
        <Typography variant="body2" sx={{ mt: 1 }}>
          {current} / {total}
        </Typography>
      )}
    </Box>
  );
}
