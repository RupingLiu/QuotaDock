import { describe, expect, it } from "vitest";
import type {
  ProviderId,
  ProviderSnapshot,
  RefreshProvidersResult,
} from "./usage";

function primaryAmount(snapshot: ProviderSnapshot): string | number | null {
  switch (snapshot.provider) {
    case "codex":
      return snapshot.data.weekly.remainingPercent;
    case "deepseek":
      return snapshot.data.balances[0]?.toppedUpBalance ?? null;
    case "kimi":
      return snapshot.data.availableBalance;
  }
}

describe("multi-provider DTOs", () => {
  it("narrows tagged snapshots without converting decimal strings", () => {
    const snapshots: ProviderSnapshot[] = [
      {
        provider: "deepseek",
        data: {
          id: "deepseek-1",
          capturedAt: "unix:1",
          isAvailable: true,
          balances: [
            {
              currency: "CNY",
              totalBalance: "110.0000",
              grantedBalance: "10.0000",
              toppedUpBalance: "100.0000",
            },
          ],
        },
      },
      {
        provider: "kimi",
        data: {
          id: "kimi-1",
          capturedAt: "unix:2",
          region: "china",
          currency: "CNY",
          availableBalance: "49.5900",
          cashBalance: "-0.4100",
          voucherBalance: "50.0000",
        },
      },
    ];

    expect(snapshots.map(primaryAmount)).toEqual(["100.0000", "49.5900"]);
  });

  it("represents partial refresh results per provider", () => {
    const providerIds: ProviderId[] = ["codex", "deepseek", "kimi"];
    const result = {
      anyUpdated: true,
      providerResults: [
        {
          providerId: "codex",
          outcome: "updated",
          message: "updated",
          errorCategory: null,
        },
        {
          providerId: "deepseek",
          outcome: "failed",
          message: "failed",
          errorCategory: "network",
        },
      ],
    } satisfies Pick<RefreshProvidersResult, "anyUpdated" | "providerResults">;

    expect(providerIds).toEqual(["codex", "deepseek", "kimi"]);
    expect(result.anyUpdated).toBe(true);
    expect(result.providerResults[1]).toMatchObject({
      providerId: "deepseek",
      outcome: "failed",
      errorCategory: "network",
    });
  });
});
