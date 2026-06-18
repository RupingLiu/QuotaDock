export type SnapshotSource = "pasted-status" | "codex-cli";

export type StorageStatus = "ready" | "missing" | "recovered" | "unsupported-version";

export type ParseWarning = {
  code: string;
  message: string;
};

export type QuotaReading = {
  remainingPercent: number | null;
  resetAt: string | null;
  resetCountdownSeconds: number | null;
};

export type QuotaSnapshot = {
  id: string;
  source: SnapshotSource;
  capturedAt: string;
  fiveHour: QuotaReading;
  weekly: QuotaReading;
  rawText: string;
  statusMessage: string;
  warnings: ParseWarning[];
};

export type ParseResult = {
  snapshot: QuotaSnapshot;
};

export type AppState = {
  version: number;
  latestSnapshot: QuotaSnapshot | null;
  storageStatus: StorageStatus;
  storagePath: string | null;
  backupPath: string | null;
  statusMessage: string;
};

export type RefreshUsageResult = {
  appState: AppState;
  updated: boolean;
  message: string;
};
