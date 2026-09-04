"use client";

import Alert from "@mui/material/Alert";
import Box from "@mui/material/Box";
import Button from "@mui/material/Button";
import Container from "@mui/material/Container";
import Paper from "@mui/material/Paper";
import Typography from "@mui/material/Typography";
import { useJob } from "../hooks/useJob";
import ChromatogramDivider from "../components/ChromatogramDivider";
import IntroSection from "../components/IntroSection";
import UploadForm from "../components/UploadForm";
import ProgressView from "../components/ProgressView";
import ResultsView from "../components/ResultsView";

export default function Home() {
  const { state, submit, reset } = useJob();

  return (
    <Container maxWidth="md" sx={{ py: 6 }}>
      <Box sx={{ mb: 1 }}>
        <Typography variant="h4" component="h1">
          RustySERRF
        </Typography>
        <Typography variant="body2" color="text.secondary">
          Remove systematic error from metabolomics batches using random forest normalization.
        </Typography>
      </Box>

      <Box sx={{ my: 3 }}>
        <ChromatogramDivider />
      </Box>

      {state.phase === "idle" && <IntroSection />}

      <Paper
        variant="outlined"
        sx={{ p: { xs: 2.5, sm: 4 }, borderColor: "divider" }}
      >
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
      </Paper>
    </Container>
  );
}
