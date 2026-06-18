export type ConfidenceState = "fresh" | "stale" | "partial" | "manual" | "unavailable";

export type SnapshotSource = "pasted-status" | "manual";

export type ManualField = "remaining-percent" | "reset-at" | "credits-balance" | "notes";

export type StorageStatus = "ready" | "missing" | "recovered" | "unsupported-version";

export type CodexProbeStatus =
  | "unknown"
  | "healthy"
  | "unavailable"
  | "not-authenticated"
  | "warning";

export type Settings = {
  staleAfterMinutes: number;
  notifyBelowPercent: number[];
  clipboardMonitoring: boolean;
};

export type ParseWarning = {
  code: string;
  message: string;
};

export type UsageSnapshot = {
  id: string;
  source: SnapshotSource;
  parsedAt: string;
  remainingPercent: number | null;
  resetAt: string | null;
  resetCountdownSeconds: number | null;
  creditsBalance: number | null;
  model: string | null;
  contextWindow: string | null;
  confidence: ConfidenceState;
  rawText: string;
  manualFields: ManualField[];
  warnings: ParseWarning[];
  notes: string;
};

export type ParseResult = {
  snapshot: UsageSnapshot;
};

export type ManualUpdateInput = {
  remainingPercent: number | null;
  resetAt: string | null;
  creditsBalance: number | null;
  notes: string | null;
};

export type CodexHealth = {
  status: CodexProbeStatus;
  available: boolean;
  authenticated: boolean | null;
  version: string | null;
  doctorStatus: string | null;
  checkedAt: string | null;
  diagnostics: string[];
};

export type AppState = {
  version: number;
  settings: Settings;
  latestSnapshot: UsageSnapshot | null;
  history: UsageSnapshot[];
  storageStatus: StorageStatus;
  storagePath: string | null;
  backupPath: string | null;
  codexHealth: CodexHealth;
};
