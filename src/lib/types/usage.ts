export type SnapshotSource = "pasted-status" | "codex-cli" | "codex-app-server";

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
  weekly: QuotaReading;
  planType: string | null;
  creditsBalance: string | null;
  resetCreditsAvailable: number | null;
  rawText: string;
  statusMessage: string;
  warnings: ParseWarning[];
};

export type ParseResult = {
  snapshot: QuotaSnapshot;
};

export type UsageHistoryPoint = {
  capturedAt: string;
  weeklyRemainingPercent: number | null;
};

export type AppSettings = {
  automaticUpdateChecks: boolean;
  lowQuotaNotifications: boolean;
};

export type RecoveryNotice = {
  status: StorageStatus;
  message: string;
  backupPath: string;
};

export type AppState = {
  version: number;
  latestSnapshot: QuotaSnapshot | null;
  storageStatus: StorageStatus;
  storagePath: string | null;
  backupPath: string | null;
  statusMessage: string;
  history: UsageHistoryPoint[];
  settings: AppSettings;
  recoveryNotice: RecoveryNotice | null;
};

export type RefreshUsageResult = {
  appState: AppState;
  updated: boolean;
  message: string;
};

export type SettingsPatch = {
  automaticUpdateChecks?: boolean;
  lowQuotaNotifications?: boolean;
};

export type AppDiagnostics = {
  appVersion: string;
  codexPath: string | null;
  codexVersion: string | null;
  latestSource: SnapshotSource | null;
  latestSuccessAt: string | null;
  storagePath: string | null;
  storageStatus: StorageStatus;
  startupEnabled: boolean;
  signedUpdatesEnabled: boolean;
};

export type UpdatePhase =
  | "idle"
  | "checking"
  | "up-to-date"
  | "available"
  | "downloading"
  | "error";

export type UpdateStatus = {
  currentVersion: string;
  phase: UpdatePhase;
  message: string;
  technicalDetail: string | null;
  availableVersion: string | null;
  progressPercent: number | null;
  checkedAt: string | null;
};
