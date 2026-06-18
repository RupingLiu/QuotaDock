import type { QuotaDockApi } from "$lib/api/tauri";
import { tauriApi } from "$lib/api/tauri";
import type { AppState, QuotaSnapshot } from "$lib/types/usage";

export class UsageState {
  appState = $state<AppState | null>(null);
  parsedDraft = $state<QuotaSnapshot | null>(null);
  pasteText = $state("");
  loading = $state(false);
  refreshing = $state(false);
  parsing = $state(false);
  saving = $state(false);
  errorMessage = $state<string | null>(null);
  noticeMessage = $state<string | null>(null);

  constructor(private readonly api: QuotaDockApi = tauriApi) {}

  get activeSnapshot(): QuotaSnapshot | null {
    return this.parsedDraft ?? this.appState?.latestSnapshot ?? null;
  }

  async load(): Promise<void> {
    await this.capture(async () => {
      this.loading = true;
      this.appState = await this.api.getAppState();
      this.noticeMessage = this.appState.statusMessage;
    }).finally(() => {
      this.loading = false;
    });
  }

  async refreshUsage(): Promise<void> {
    await this.capture(async () => {
      this.refreshing = true;
      const result = await this.api.refreshUsage();
      this.appState = result.appState;
      this.parsedDraft = null;
      this.noticeMessage = result.message;
    }).finally(() => {
      this.refreshing = false;
    });
  }

  async parseStatusText(): Promise<void> {
    const rawText = this.pasteText.trim();
    if (!rawText) {
      this.noticeMessage = "请先粘贴 /status 内容。";
      return;
    }

    await this.capture(async () => {
      this.parsing = true;
      const result = await this.api.parseStatusText(rawText);
      this.parsedDraft = result.snapshot;
      this.noticeMessage = result.snapshot.statusMessage;
    }).finally(() => {
      this.parsing = false;
    });
  }

  async saveParsedDraft(): Promise<void> {
    if (!this.parsedDraft) {
      this.noticeMessage = "没有可保存的解析结果。";
      return;
    }

    await this.capture(async () => {
      this.saving = true;
      this.appState = await this.api.saveSnapshot(this.parsedDraft as QuotaSnapshot);
      this.parsedDraft = null;
      this.pasteText = "";
      this.noticeMessage = this.appState.statusMessage;
    }).finally(() => {
      this.saving = false;
    });
  }

  async clearSnapshot(): Promise<void> {
    await this.capture(async () => {
      this.saving = true;
      this.appState = await this.api.clearSnapshot();
      this.parsedDraft = null;
      this.pasteText = "";
      this.noticeMessage = "已清空额度快照。";
    }).finally(() => {
      this.saving = false;
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
