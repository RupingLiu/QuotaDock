# QuotaDock Design System

**Product shape:** Glanceable Windows desktop status strip
**Primary surface:** `360 × 36px` floating bar
**Secondary surfaces:** `440 × 600px` on-demand details panel + native tray/context menu
**Style:** Compact, calm, trustworthy, data-first

## Design Principles

1. Percentage is the primary visual signal; label and reset time are secondary.
2. Always show whether data is fresh, stale, incomplete, loading, or failed.
3. Preserve the last successful values after failure, but never present them as live.
4. Keep the strip useful without hover. Tooltips may add detail, not carry essential state.
5. Support keyboard access, reduced motion, light/dark mode, and desktop text scaling.

## Layout

```text
[state]  5小时 72% 2h14m  |  1周 46% 6/23 09:00  [3m] [...]
```

- Default size: `360 × 36px`; minimum supported width: `300px`.
- Grid: status dot, two equal quota groups, freshness, menu button.
- Outer padding: `8px` horizontal, `3px` vertical.
- Menu hit target: `24 × 24px`.
- Below `320px`, hide reset time but keep the short freshness/error state visible.
- The full non-interactive surface remains draggable; the menu button is not a drag target.

## Color Tokens

| Role | Light | Dark |
|---|---|---|
| Surface | `rgba(255,255,255,.985)` | `rgba(15,23,42,.97)` |
| Primary text | `#172033` | `#F8FAFC` |
| Secondary text | `#475569` | `#C2CEDB` |
| Border | `rgba(100,116,139,.26)` | `rgba(148,163,184,.34)` |
| Fresh | `#0F766E` | `#2DD4BF` |
| Busy | `#1D4ED8` | `#60A5FA` |
| Warning/stale | `#92400E` | `#FDBA74` |
| Error | `#B42318` | `#FDA29B` |
| Focus | `#2563EB` | `#60A5FA` |

Warnings must use both color and a visible state marker/text. Low quota uses the darker
warning color and a literal `!`, not orange alone.

## Typography

- Use the local system stack: Segoe UI Variable/Text, Segoe UI, Microsoft YaHei UI.
- Do not download web fonts; QuotaDock must start quickly and work offline.
- Percentage: `14px`, weight `680`, tabular numerals.
- Label/reset: `11.5–12px`, weight `500–600`.
- Freshness: `10.5–11px`, weight `600`, tabular numerals.
- Never render user-facing data below `10.5px`.

## Interaction

- Right click anywhere opens the native menu.
- The trailing SVG ellipsis button exposes the same menu to mouse and keyboard users.
- Hover transitions are `150–200ms` and never change layout dimensions.
- Focus uses a visible `2px` outline.
- Busy state may pulse only the status dot; disable it under `prefers-reduced-motion`.
- Dynamic announcements use a polite live region.

## Details Panel

- The panel is an explanation and control surface, not a second always-on dashboard.
- Use a calm off-white canvas, white cards, teal trust/accent color, and a single-column
  reading order suitable for a fixed `440 × 600px` Windows window.
- Reading order: current status → recovery alert → two quota cards → provenance metadata
  → optional account summary → small trend → settings → diagnostics/official link.
- Quota cards use percent, progress, and reset time. The small trend shows at most two
  clearly labeled series and never invents forecasts.
- Settings use native semantic checkboxes styled as compact switches; every row includes
  a one-line consequence description.
- Long filesystem paths are truncated visually but retained in `title` for inspection.
- Recovery alerts remain visible until explicit acknowledgement.
- The official Usage Dashboard is labeled as external authority rather than presented as
  QuotaDock-owned data.

## State Model

| State | Visible treatment |
|---|---|
| Fresh | Teal dot + compact age (`刚刚`, `3m`, `1h`) |
| Loading | Blue pulsing dot + `读取` |
| Failed | Red dot/border + `失败`; retain last values |
| Stale | Brown warning dot/border + `陈旧` |
| Partial/recovered | Brown warning dot/border + `注意` |
| Empty | Slate dot + `等待` + unknown values |
| Low quota | Dark warning value + visible `!` |

Countdown values are derived from `capturedAt + resetCountdownSeconds`, refreshed every
30 seconds, and switch to `待刷新` after expiry.

## Anti-patterns

- No marketing hero, CTA page, navigation shell, chart grid, or generic analytics dashboard.
- No essential status available only through `title`/hover.
- No static countdown presented as current.
- No color-only warnings, emoji icons, low-contrast gray, or layout-shifting hover effects.
- No full dashboard squeezed into the always-on-top strip; future details belong in an
  on-demand panel.

## Delivery Checklist

- [ ] 360×36 and 300×36 have no horizontal/vertical overflow.
- [ ] Percentages, state, and menu remain visible at the narrow breakpoint.
- [ ] Light and dark themes maintain readable contrast.
- [ ] Fresh, busy, stale, error, partial, empty, and low-quota states are distinct.
- [ ] Menu is keyboard reachable and has a visible focus ring.
- [ ] Live state changes are announced.
- [ ] `prefers-reduced-motion` is respected.
- [ ] Hover/focus never moves neighboring content.
