export type JobEvent =
  | { status: "queued" }
  | { status: "progress"; stage: string; current: number; total: number }
  | { status: "completed" }
  | { status: "failed"; error: string };

export interface PcaJson {
  pc1: number[];
  pc2: number[];
  sample_type: (string | null)[];
  batch: string[];
}

export interface ResultJson {
  compound_labels: string[];
  qc_rsd_raw: number[];
  qc_rsd_serrf: number[];
  validate_rsd_raw: Record<string, number[]>;
  validate_rsd_serrf: Record<string, number[]>;
  pca_before: PcaJson;
  pca_after: PcaJson;
}
