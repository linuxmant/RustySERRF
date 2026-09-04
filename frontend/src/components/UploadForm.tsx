"use client";

import { useState } from "react";
import Box from "@mui/material/Box";
import Button from "@mui/material/Button";
import Typography from "@mui/material/Typography";

interface UploadFormProps {
  onSubmit: (file: File) => void;
}

export default function UploadForm({ onSubmit }: UploadFormProps) {
  const [file, setFile] = useState<File | null>(null);

  return (
    <Box
      component="form"
      onSubmit={(event) => {
        event.preventDefault();
        if (file) {
          onSubmit(file);
        }
      }}
    >
      <Typography variant="h5" gutterBottom>
        Upload a dataset
      </Typography>
      <input
        type="file"
        accept=".csv,.xlsx"
        aria-label="dataset file"
        onChange={(event) => setFile(event.target.files?.[0] ?? null)}
      />
      <Box sx={{ mt: 2 }}>
        <Button type="submit" variant="contained" disabled={!file}>
          Run SERRF normalization
        </Button>
      </Box>
    </Box>
  );
}
