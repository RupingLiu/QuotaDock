# QuotaDock MVP Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a Windows-first Tauri desktop utility that tracks personal Codex usage from pasted `/status` output, local manual entries, and local Codex CLI health checks without using undocumented account APIs.

**Architecture:** The Svelte/TypeScript frontend renders the compact dashboard and calls a narrow Tauri command API. Rust owns parsing, JSON persistence, Codex CLI probing, tray actions, notifications, official links, and redaction. MVP state is a versioned JSON file under the Tauri app data directory, with bounded snapshot history.

**Tech Stack:** Tauri v2, Rust, Svelte, TypeScript, Vite, npm, Vitest, Cargo tests, `serde`, `serde_json`, `regex`, `chrono`, `uuid`, `thiserror`, `tempfile`, `tauri-plugin-opener`, `tauri-plugin-notification`, `@tauri-apps/api`, and `@tauri-apps/plugin-opener`.

---

## File Structure

- `package.json`, `vite.config.ts`, `svelte.config.js`, `tsconfig*.json`, `index.html`: npm/Svelte/Vite/Tauri frontend scaffold and scripts.
- `src/main.ts`, `src/App.svelte`, `src/app.css`: frontend entrypoint, root utility layout, and compact app styling.
- `src/lib/types/usage.ts`: TypeScript mirror of the Rust command DTOs.
- `src/lib/api/tauri.ts`: all frontend calls to Tauri commands.
- `src/lib/state/usageState.svelte.ts`: frontend state orchestration for app load, parse/save, manual updates, probe refresh, settings, and reset.
- `src/lib/utils/freshness.ts`: freshness and display helpers with Vitest coverage.
- `src/lib/components/*.svelte`: utility dashboard, confidence badge, health panel, paste/manual flows, history, and settings.
- `src-tauri/src/main.rs`: thin binary entrypoint.
- `src-tauri/src/lib.rs`: Tauri builder, plugin setup, state wiring, command registration, tray setup.
- `src-tauri/src/models.rs`: command DTOs, storage structs, confidence/source enums, default settings.
- `src-tauri/src/usage_store.rs`: versioned JSON read/write, safe recovery metadata, backup/reset, bounded history.
- `src-tauri/src/status_parser.rs`: conservative `/status` parser and confidence calculation.
- `src-tauri/src/codex_probe.rs`: fixed Codex CLI health probes with timeouts and mockable runner.
- `src-tauri/src/redaction.rs`: display-safe diagnostic redaction.
- `src-tauri/src/official_links.rs`: centralized OpenAI/Codex links.
- `src-tauri/src/tray.rs`: tray menu, show-window, probe-refresh, official link, quit.
- `src-tauri/src/reminder.rs`: low-usage and stale snapshot notification decisions.
- `src-tauri/src/commands.rs`: Tauri command handlers.
- `src-tauri/fixtures/**`: parser, store, and probe fixtures used by Rust tests.

## Public Interfaces

Tauri commands to expose:

```text
get_app_state() -> AppState
parse_status_text(raw_text: String) -> ParseResult
save_snapshot(snapshot: UsageSnapshot) -> AppState
update_manual_fields(input: ManualUpdateInput) -> AppState
refresh_codex_probe() -> CodexHealth
open_official_usage() -> Result<(), AppError>
update_settings(settings: Settings) -> AppState
backup_and_reset_store() -> AppState
```

The official personal usage dashboard URL is centralized as:

```text
https://chatgpt.com/codex/settings/usage
```

## Task 1: Scaffold Tauri/Svelte Baseline

**Files:**
- Create/modify root Tauri/Svelte scaffold files.
- Create/modify `src-tauri/tauri.conf.json`, `src-tauri/Cargo.toml`, `src-tauri/src/main.rs`, `src-tauri/src/lib.rs`.
- Modify `README.md`.

- [ ] Create a Tauri v2 Svelte TypeScript app in the worktree using npm.
- [ ] Configure app id `com.rupingliu.quotadock`, product name `QuotaDock`, and dev URL `http://localhost:5173`.
- [ ] Add scripts: `dev`, `build`, `preview`, `test`, `tauri`.
- [ ] Add a placeholder `get_app_state` Tauri command returning an unavailable state so frontend and Tauri wiring compile.
- [ ] Add README commands for `npm install`, `npm run dev`, `npm run test`, `npm run tauri dev`, and `npm run tauri build`.
- [ ] Verify `npm install`, `npm run build`, and `cargo test --manifest-path src-tauri/Cargo.toml`.
- [ ] Commit with `chore: scaffold tauri svelte app`.

## Task 2: Store And Shared Models

**Files:**
- Create/modify `src-tauri/src/models.rs`, `src-tauri/src/usage_store.rs`, `src-tauri/src/commands.rs`.
- Create fixtures under `src-tauri/fixtures/storage/`.
- Create/modify `src/lib/types/usage.ts`.

- [ ] Write failing Rust tests for missing storage file, valid v1 file, corrupt JSON recovery, unsupported schema version, history truncation to 100, and backup/reset.
- [ ] Implement shared Rust DTOs and JSON defaults: `Settings`, `UsageSnapshot`, `ManualFields`, `CodexHealth`, `ConfidenceState`, `ParseWarning`, `AppState`.
- [ ] Implement `UsageStore` using Tauri app data directory in production and injectable paths in tests.
- [ ] Keep corrupt data by backing it up before reset; never silently discard data.
- [ ] Mirror DTOs in TypeScript.
- [ ] Verify `cargo test --manifest-path src-tauri/Cargo.toml usage_store`.
- [ ] Commit with `feat: add usage store and models`.

## Task 3: Conservative Status Parser

**Files:**
- Create/modify `src-tauri/src/status_parser.rs`, `src-tauri/src/models.rs`, `src-tauri/src/commands.rs`.
- Create fixtures under `src-tauri/fixtures/status/`.

- [ ] Write failing Rust parser tests for complete status text, missing reset, missing percent, reordered lines, unrelated noise, unknown format, credits balance, model extraction, and manual overlay.
- [ ] Parse model, remaining percentage, reset timestamp or countdown, credits balance, context/token capacity when present, raw text, parse timestamp, warnings, and manual field markers.
- [ ] Preserve unknown raw lines and return warnings instead of failing the whole parse.
- [ ] Calculate confidence conservatively: `manual` when important fields are manually supplied, `partial` when useful fields are missing, `unavailable` when nothing meaningful is parsed, and `fresh` only for a recent usable snapshot.
- [ ] Expose `parse_status_text` and wire save flow through the store.
- [ ] Verify `cargo test --manifest-path src-tauri/Cargo.toml status_parser`.
- [ ] Commit with `feat: parse codex status snapshots`.

## Task 4: Codex Health Probe

**Files:**
- Create/modify `src-tauri/src/codex_probe.rs`, `src-tauri/src/redaction.rs`, `src-tauri/src/models.rs`, `src-tauri/src/commands.rs`.
- Create fixtures under `src-tauri/fixtures/probe/`.

- [ ] Write failing Rust tests for Codex missing, version success, login signed-in, login signed-out, command failure, doctor success JSON, doctor warning JSON, invalid doctor JSON, timeout, and redaction.
- [ ] Implement a mockable command runner with fixed allowlisted commands only.
- [ ] Run `codex --version`, `codex login status`, and `codex doctor --json` with short timeouts.
- [ ] Return display-safe diagnostics and never read auth/token files.
- [ ] Expose `refresh_codex_probe`.
- [ ] Verify `cargo test --manifest-path src-tauri/Cargo.toml codex_probe redaction`.
- [ ] Commit with `feat: add codex health probe`.

## Task 5: Dashboard UI And Frontend State

**Files:**
- Create/modify `src/App.svelte`, `src/app.css`, `src/lib/api/tauri.ts`, `src/lib/state/usageState.svelte.ts`, `src/lib/utils/freshness.ts`.
- Create components under `src/lib/components/`.
- Create frontend tests next to the relevant utilities/components.

- [ ] Write failing Vitest tests for freshness helpers and confidence display labels.
- [ ] Implement frontend command adapter with typed command names matching Rust.
- [ ] Implement app state loading, parse/save flow, manual update flow, settings update, probe refresh, and backup/reset calls.
- [ ] Build a compact utility dashboard with no marketing hero page.
- [ ] Show remaining percent, reset time/countdown, last updated, confidence state, manual markers, parse warnings, raw text, notes, history, settings, and local Codex health.
- [ ] Ensure paste/manual flows stay usable when Codex CLI is unavailable.
- [ ] Verify `npm run test` and `npm run build`.
- [ ] Commit with `feat: build quotadock dashboard`.

## Task 6: Tray, Links, And Notifications

**Files:**
- Create/modify `src-tauri/src/tray.rs`, `src-tauri/src/reminder.rs`, `src-tauri/src/official_links.rs`, `src-tauri/src/lib.rs`, `src-tauri/tauri.conf.json`, `src-tauri/capabilities/default.json`, `src-tauri/Cargo.toml`, `package.json`.

- [ ] Write failing Rust tests for reminder decisions: below threshold, threshold already notified, stale snapshot, stale unavailable ignored, and reset of notification state after fresh update.
- [ ] Add `tauri-plugin-opener` and `tauri-plugin-notification`.
- [ ] Implement tray menu entries: open QuotaDock, paste/status focus action, open official Usage Dashboard, refresh local Codex checks, quit.
- [ ] Implement `open_official_usage` through centralized official links.
- [ ] Implement reminder decisions so low-usage warnings never fire for stale or unavailable data.
- [ ] Verify `cargo test --manifest-path src-tauri/Cargo.toml reminder official_links` and `npm run build`.
- [ ] Commit with `feat: add tray links and reminders`.

## Task 7: Verification And Packaging

**Files:**
- Modify `README.md`.
- Optionally add `docs/superpowers/manual-verification/2026-06-18-windows-smoke.md`.

- [ ] Run `npm run test`.
- [ ] Run `cargo test --manifest-path src-tauri/Cargo.toml`.
- [ ] Run `npm run build`.
- [ ] Run `npm run tauri build`.
- [ ] Manually smoke test on Windows: app launch, tray appears, tray opens main window, paste flow updates dashboard, manual fallback stores fields as manual, official usage link opens, notifications can be enabled/disabled.
- [ ] Record verification results and package path in README or a manual verification note.
- [ ] Commit with `test: verify quotadock mvp`.

## Final Review

- [ ] Dispatch a final code reviewer subagent against the full branch.
- [ ] Fix all Critical and Important issues.
- [ ] Re-run `npm run test`, `cargo test --manifest-path src-tauri/Cargo.toml`, `npm run build`, and `npm run tauri build`.
- [ ] Use `superpowers:finishing-a-development-branch` to offer merge/PR/keep/discard options after verification passes.

