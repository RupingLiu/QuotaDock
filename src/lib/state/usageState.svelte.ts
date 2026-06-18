import type { QuotaDockApi } from "$lib/api/tauri";
import { tauriApi } from "$lib/api/tauri";
import type { AppState, ManualUpdateInput, Settings, UsageSnapshot } from "$lib/types/usage";

export class UsageState {
  appState = $state<AppState | null>(null);
  parsedDraft = $state<UsageSnapshot | null>(null);
  loading = $state(false);
  parsing = $state(false);
  saving = $state(false);
  probing = $state(false);
  errorMessage = $state<string | null>(null);

  constructor(private readonly api: QuotaDockApi = tauriApi) {}

  get latestSnapshot(): UsageSnapshot | null {
    return this.appState?.latestSnapshot ?? null;
  }

  get settings(): Settings | null {
    return this.appState?.settings ?? null;
  }

  get history(): UsageSnapshot[] {
    return this.appState?.history ?? [];
  }

  async load(): Promise<void> {
    await this.capture(async () => {
      this.loading = true;
      this.appState = await this.api.getAppState();
    }).finally(() => {
      this.loading = false;
    });
  }

  async parseStatusText(rawText: string): Promise<void> {
    await this.capture(async () => {
      this.parsing = true;
      const result = await this.api.parseStatusText(rawText);
      this.parsedDraft = result.snapshot;
    }).finally(() => {
      this.parsing = false;
    });
  }

  async saveParsedDraft(): Promise<void> {
    if (!this.parsedDraft) {
      return;
    }
    await this.capture(async () => {
      this.saving = true;
      this.appState = await this.api.saveSnapshot(this.parsedDraft as UsageSnapshot);
      this.parsedDraft = null;
    }).finally(() => {
      this.saving = false;
    });
  }

  async updateManualFields(input: ManualUpdateInput): Promise<void> {
    await this.capture(async () => {
      this.saving = true;
      this.appState = await this.api.updateManualFields(input);
    }).finally(() => {
      this.saving = false;
    });
  }

  async refreshProbe(): Promise<void> {
    await this.capture(async () => {
      this.probing = true;
      const codexHealth = await this.api.refreshCodexProbe();
      if (this.appState) {
        this.appState = { ...this.appState, codexHealth };
      }
    }).finally(() => {
      this.probing = false;
    });
  }

  async updateSettings(settings: Settings): Promise<void> {
    await this.capture(async () => {
      this.saving = true;
      this.appState = await this.api.updateSettings(settings);
    }).finally(() => {
      this.saving = false;
    });
  }

  async backupAndResetStore(): Promise<void> {
    await this.capture(async () => {
      this.saving = true;
      this.appState = await this.api.backupAndResetStore();
      this.parsedDraft = null;
    }).finally(() => {
      this.saving = false;
    });
  }

  async openOfficialUsage(): Promise<void> {
    await this.capture(async () => {
      await this.api.openOfficialUsage();
    });
  }

  private async capture(work: () => Promise<void>): Promise<void> {
    this.errorMessage = null;
    try {
      await work();
    } catch (error) {
      this.errorMessage = error instanceof Error ? error.message : String(error);
    }
  }
}

export function createUsageState(api?: QuotaDockApi): UsageState {
  return new UsageState(api);
}
