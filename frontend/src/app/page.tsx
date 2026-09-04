"use client";

import Alert from "@mui/material/Alert";
import Box from "@mui/material/Box";
import Button from "@mui/material/Button";
import Container from "@mui/material/Container";
import Typography from "@mui/material/Typography";
import { useJob } from "../hooks/useJob";
import ThemeToggle from "../components/ThemeToggle";
import UploadForm from "../components/UploadForm";
import ProgressView from "../components/ProgressView";
import ResultsView from "../components/ResultsView";

export default function Home() {
  const { state, submit, reset } = useJob();

  return (
    <Container maxWidth="lg" sx={{ py: 4 }}>
      <Box sx={{ display: "flex", justifyContent: "space-between", alignItems: "center", mb: 3 }}>
        <Typography variant="h4" component="h1">
          RustySERRF
        </Typography>
        <ThemeToggle />
      </Box>

      {state.phase === "idle" && <UploadForm onSubmit={submit} />}
      {state.phase === "uploading" && <ProgressView />}
      {state.phase === "processing" && (
        <ProgressView stage={state.stage} current={state.current} total={state.total} />
      )}
      {state.phase === "done" && <ResultsView jobId={state.jobId} result={state.result} onReset={reset} />}
      {state.phase === "error" && (
        <Box>
          <Alert severity="error" sx={{ mb: 2 }}>
            {state.message}
          </Alert>
          <Button variant="outlined" onClick={reset}>
            Start over
          </Button>
        </Box>
      )}
    </Container>
  );
}
