import type { ConfidenceState, Settings, UsageSnapshot } from "$lib/types/usage";

export type FreshnessView = {
  state: ConfidenceState;
  ageMinutes: number | null;
  isStale: boolean;
  label: string;
};

const STATE_LABELS: Record<ConfidenceState, string> = {
  fresh: "Fresh",
  stale: "Stale",
  partial: "Partial",
  manual: "Manual",
  unavailable: "Unavailable",
};

export function evaluateSnapshotFreshness(
  snapshot: UsageSnapshot | null,
  settings: Settings,
  now: Date = new Date(),
): FreshnessView {
  if (!snapshot) {
    return { state: "unavailable", ageMinutes: null, isStale: false, label: "Unavailable" };
  }

  if (snapshot.confidence === "manual") {
    return { state: "manual", ageMinutes: ageMinutes(snapshot.parsedAt, now), isStale: false, label: "Manual" };
  }

  if (snapshot.confidence === "partial" || snapshot.confidence === "unavailable") {
    return {
      state: snapshot.confidence,
      ageMinutes: ageMinutes(snapshot.parsedAt, now),
      isStale: false,
      label: STATE_LABELS[snapshot.confidence],
    };
  }

  const age = ageMinutes(snapshot.parsedAt, now);
  const isStale = age === null || age > settings.staleAfterMinutes;
  const state: ConfidenceState = isStale ? "stale" : "fresh";

  return {
    state,
    ageMinutes: age,
    isStale,
    label: STATE_LABELS[state],
  };
}

export function formatAge(age: number | null): string {
  if (age === null) {
    return "unknown age";
  }
  if (age < 1) {
    return "just now";
  }
  if (age === 1) {
    return "1 min ago";
  }
  return `${age} min ago`;
}

export function confidenceLabel(state: ConfidenceState): string {
  return STATE_LABELS[state];
}

function ageMinutes(parsedAt: string, now: Date): number | null {
  const value = timestampMillis(parsedAt);
  if (Number.isNaN(value)) {
    return null;
  }
  return Math.max(0, Math.floor((now.getTime() - value) / 60000));
}

function timestampMillis(value: string): number {
  if (value.startsWith("unix:")) {
    return Number(value.slice("unix:".length)) * 1000;
  }
  return new Date(value).getTime();
}
