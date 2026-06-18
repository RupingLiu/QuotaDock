# QuotaDock: Codex Usage Desktop Tool Design

Date: 2026-06-18

## Summary

QuotaDock is a lightweight desktop utility for tracking a user's personal Codex usage state. It is aimed at ChatGPT Plus/Pro personal use, not Enterprise workspace analytics and not OpenAI API billing.

The product should be honest about data quality. Personal Codex usage is officially visible through the Codex Usage Dashboard and, during an active Codex CLI session, `/status`. QuotaDock should not depend on an undocumented personal subscription API. The MVP will therefore use a semi-automatic workflow: inspect local Codex installation health, parse pasted `/status` output, store a local snapshot, and provide quick links back to the official Usage Dashboard for confirmation.

Official references:

- Codex Pricing and usage limits: https://developers.openai.com/codex/pricing
- Codex Governance and Enterprise Analytics API: https://developers.openai.com/codex/enterprise/governance
- Using Codex with a ChatGPT plan: https://help.openai.com/en/articles/11369540-using-codex-with-your-chatgpt-plan

## Goals

- Show a compact personal Codex usage summary in a desktop window.
- Provide a tray entry for quick access without keeping a full app window open.
- Parse user-pasted Codex `/status` output into a structured local snapshot.
- Show when data was last updated and whether it is fresh, stale, partial, or manually entered.
- Offer a quick path to the official Codex Usage Dashboard for verification.
- Allow the user to enable or disable launch at Windows startup.
- Keep the first implementation Windows-first while leaving a realistic path to macOS/Linux through Tauri.

## Non-Goals

- Do not bypass, reset, extend, or work around Codex usage limits.
- Do not scrape private OpenAI account pages in the MVP.
- Do not claim real-time precision when the app only has a pasted snapshot.
- Do not implement Enterprise workspace analytics in the personal MVP.
- Do not track OpenAI Platform API billing as part of this first scope.

## Target Platform

The first version targets Windows because that is the current user environment and it makes tray integration, packaging, and local verification concrete. The architecture should remain portable by using Tauri APIs and isolating platform-specific behavior.

Recommended stack:

- Tauri for the desktop shell, tray, notifications, filesystem access, and command execution.
- Svelte with TypeScript for a lightweight UI.
- Rust/Tauri commands for local probing, parsing, persistence, notifications, and safe OS integration.
- JSON file storage for MVP state. SQLite can be introduced later if history or querying becomes more complex.

## Product Shape

QuotaDock has two user-facing surfaces:

- Main window: the primary status dashboard and settings surface.
- System tray: quick status entry with actions to open the window, paste/parse status, open official Usage Dashboard, refresh local Codex checks, and quit.

The main window should feel like a utility, not a landing page. The first screen should show:

- Current estimated remaining usage percentage, if known.
- Reset time or countdown, if parsed.
- Last updated timestamp.
- Data confidence state: fresh, stale, partial, manual, or unavailable.
- Primary actions: paste `/status`, manually edit, open official Usage Dashboard.

Details should be one level deeper:

- Parsed model and surface details.
- Credits balance if the pasted text or manual entry includes it.
- Raw pasted text for auditability.
- Manual notes.
- History of recent snapshots.

Settings should include:

- Stale-data threshold.
- Notification thresholds.
- Notification enable/disable controls.
- Clipboard monitoring toggle, disabled by default.
- Launch at startup toggle, disabled by default.

## Data Sources

### Local Codex Health Probe

QuotaDock should probe local Codex health without reading secrets:

- `codex --version`
- `codex login status`
- `codex doctor --json`

This module only answers whether the local Codex installation and authentication appear healthy. It does not determine remaining usage by itself.

### Pasted `/status` Output

The MVP's primary usage source is pasted `/status` output from Codex. The parser should extract what it can and preserve the raw text.

Expected fields include:

- Model name, if present.
- Rate limit or usage-limit fields, if present.
- Remaining usage or percentage, if present.
- Reset time or reset countdown, if present.
- Context or token capacity fields, if present.
- Source timestamp from the local machine when parsed.

Because the exact `/status` format may change, parsing must be resilient and conservative. Unknown lines should not fail the whole parse.

### Parser Fixture Requirements

The first parser implementation needs real, sanitized fixture samples before it can be considered reliable. Fixtures should live under `tests/fixtures/status/` once the app scaffold exists.

Required fixture classes:

- A complete `/status` output that includes model, rate limits, remaining usage, reset timing, and context details.
- A partial `/status` output that lacks at least one important usage field.
- A status output from an unavailable or unusual state, such as not signed in, API-key mode, or no rate-limit details shown.
- A manually created future-format sample with extra unrelated lines and changed ordering.

Fixture hygiene:

- Remove tokens, account identifiers, private paths, and personal email addresses before committing samples.
- Keep realistic wording and spacing so parser tests exercise the real shape of the output.
- Store the expected parsed result next to each fixture as JSON.

### Manual Entry

Manual entry is the fallback when pasted text is incomplete or cannot be parsed. Users may manually enter:

- Remaining percentage.
- Reset time.
- Credits balance.
- Notes.

Manual fields must be marked as manual in the UI and in the saved snapshot.

### Clipboard Monitoring

Clipboard monitoring is optional and off by default. A later version may watch for copied `/status` output and offer to parse it. It must:

- Require explicit opt-in.
- Show a privacy note explaining what is monitored.
- Avoid uploading clipboard content.
- Only parse locally.

## Data Model

MVP storage can be a JSON file under the app data directory.

Suggested shape:

```json
{
  "version": 1,
  "settings": {
    "staleAfterMinutes": 60,
    "notifyBelowPercent": [20, 10],
    "clipboardMonitoring": false,
    "launchAtStartup": false
  },
  "latestSnapshot": {
    "id": "2026-06-18T04:55:00Z",
    "source": "pasted-status",
    "parsedAt": "2026-06-18T04:55:00Z",
    "remainingPercent": 72,
    "resetAt": "2026-06-18T07:10:00Z",
    "creditsBalance": null,
    "model": "gpt-5.5",
    "confidence": "partial",
    "rawText": "...",
    "manualFields": [],
    "notes": ""
  },
  "history": []
}
```

The app should tolerate a missing or corrupt storage file by starting in an unavailable state and offering to create a new state file. It should not crash or silently discard data.

## Storage Location and Backup

MVP state should be stored in the user app-data directory, not beside the executable and not in the project workspace.

Default Windows layout:

- State file: `%APPDATA%\QuotaDock\state.json`
- Backups: `%APPDATA%\QuotaDock\backups\`
- Logs: `%APPDATA%\QuotaDock\logs\`

When `state.json` is corrupt or cannot be parsed, QuotaDock should:

1. Move the unreadable file to `backups/state-corrupt-<timestamp>.json`.
2. Start with an unavailable snapshot state.
3. Show a clear recovery message.
4. Allow the user to paste `/status` or manually enter values to create a fresh state file.

The app should keep at most 10 automatic corrupt-state backups by default.

## Core Modules

### CodexProbe

Responsibilities:

- Locate the `codex` executable on PATH.
- Read Codex CLI version.
- Run login status.
- Run `doctor --json` and summarize health.
- Return redacted, display-safe diagnostics.

Non-responsibilities:

- Do not read private token files directly.
- Do not calculate remaining usage.

### StatusParser

Responsibilities:

- Parse pasted `/status` output into a structured snapshot.
- Preserve raw text.
- Report parse warnings.
- Mark confidence as fresh, partial, manual, stale, or unavailable.
- Provide unit tests for representative status-output formats.

### UsageStore

Responsibilities:

- Read and write JSON state.
- Validate schema version.
- Recover safely from missing or corrupt files.
- Maintain latest snapshot and bounded history.

### TrayController

Responsibilities:

- Show a tray icon and menu.
- Open the main window.
- Trigger paste/parse flow.
- Open official Usage Dashboard.
- Refresh local Codex checks.
- Quit the app.

### StartupController

Responsibilities:

- Read whether QuotaDock is registered to launch at Windows startup.
- Enable startup launch only after the user explicitly turns it on.
- Disable startup launch when the user turns it off.
- Detect and report registration failures.
- Avoid requiring administrator privileges.

Non-responsibilities:

- Do not silently enable startup during install or first launch.
- Do not keep stale startup entries if the executable path changes and the app can safely repair or remove them.

### ReminderEngine

Responsibilities:

- Notify when known remaining usage crosses configured thresholds.
- Notify when the snapshot becomes stale.
- Avoid repeated notification spam.
- Avoid low-usage warnings when data is stale or unavailable.

Default notification behavior:

- Below 20%: notify once per snapshot.
- Below 10%: notify once per snapshot, even if the 20% notification already fired.
- Stale snapshot: notify once when the snapshot crosses the configured stale threshold.
- Reset timing: if a reset time is known, optionally notify once within 10 minutes after reset so the user can refresh status.
- Cooldown: never send the same notification type more than once per hour unless a new snapshot is parsed.
- User control: all notification classes must be individually disableable.

### OfficialLinks

Responsibilities:

- Open official Codex Usage Dashboard.
- Keep source links centralized so they can be updated later.

## User Flows

### First Run

1. User opens QuotaDock.
2. App checks whether `codex` is installed and authenticated.
3. App shows "No usage snapshot yet".
4. User clicks "Paste /status".
5. App parses the text, saves a snapshot, and updates the dashboard.

### Normal Check

1. User copies `/status` output from Codex.
2. User opens QuotaDock from tray.
3. User clicks paste/parse or pastes into the text box.
4. App updates the summary and records history.

### Manual Fallback

1. Parser cannot extract remaining usage or reset time.
2. App shows parse warnings and raw text.
3. User enters missing fields manually.
4. App stores those fields as manual.

### Official Verification

1. User clicks "Open official Usage".
2. App opens the Codex Usage Dashboard in the default browser.
3. User can compare the official page with the local snapshot.

### Launch at Startup Setup

1. User opens Settings.
2. User turns on "Launch at startup".
3. App registers QuotaDock with the current user's startup entries.
4. App shows whether registration succeeded.
5. User can return to Settings and turn the option off.

## UI States

- Unconfigured: no snapshot exists.
- Fresh: recent parsed or manually updated snapshot exists.
- Partial: some useful fields parsed, but at least one important field is missing.
- Manual: important fields are user-entered.
- Stale: snapshot age exceeds the configured threshold.
- Local Codex unavailable: `codex` is not installed, not on PATH, or not authenticated.
- Parse failed: pasted text could not be understood.
- Startup registration unavailable: the app cannot read or write the current user's startup setting.

The UI must make these states visible. It should avoid presenting stale or partial values as exact current truth.

## Error Handling

- Missing `codex`: show install/configuration hint and keep manual usage tracking available.
- `codex login status` indicates not signed in: show login status and offer official usage link.
- `doctor --json` fails: show a redacted diagnostic summary and continue with manual/paste flows.
- Parse failure: preserve raw text, show warning, and offer manual entry.
- Corrupt local JSON: offer backup-and-reset behavior.
- Notification failure: log locally and keep the app usable.
- Startup registration failure: keep the toggle off, show a clear error, and do not retry repeatedly in the background.

## Security and Privacy

- Do not store OpenAI account passwords or API keys.
- Do not read Codex auth token files directly.
- Do not upload pasted `/status` text.
- Keep clipboard monitoring off by default.
- Keep launch at startup off by default.
- Redact command diagnostics before showing or saving them if they contain paths or account identifiers that are not needed.
- Label the app as an unofficial personal utility, not an OpenAI product.

## Testing Plan

Focus early testing on parser and state behavior:

- Parser tests for complete `/status` samples.
- Parser tests for missing fields.
- Parser tests for changed line order.
- Parser tests for extra unrelated lines.
- Storage tests for missing, valid, and corrupt JSON.
- Probe tests for `codex` not found, command failure, and success responses.
- UI state tests for fresh, partial, manual, stale, and unavailable snapshots.

Manual verification on Windows:

- App launches.
- Tray menu appears.
- Main window opens from tray.
- Paste flow updates dashboard.
- Official Usage Dashboard link opens.
- Notifications can be enabled and disabled.
- Launch at startup can be enabled and disabled without administrator privileges.

## Windows Packaging

The first packaged build should optimize for quick personal use on Windows:

- Primary deliverable: portable `.exe` build for local use.
- Secondary deliverable: installer package after the tray, storage, and notification flows are stable.
- The portable build should create app-data directories on first launch.
- The app should not require administrator privileges.
- Startup registration should be per-user and should not require administrator privileges.
- If the portable executable is moved after startup registration, the app should detect the mismatch on next launch and ask the user to re-enable startup.
- The package should include version metadata and the QuotaDock product name.
- Code signing is optional for early private builds and should be revisited before public distribution.

## Acceptance Criteria

MVP is complete when all of the following are true:

- The app launches on Windows and shows a compact main window.
- The tray icon appears and can reopen the main window after it is closed or hidden.
- The app can run `codex --version`, `codex login status`, and `codex doctor --json`, then show a redacted health summary.
- The user can paste a `/status` sample and get a saved usage snapshot.
- The main window shows remaining percentage, reset timing, last updated time, and confidence state when those fields are known.
- Missing fields are shown as unknown instead of guessed.
- The user can manually enter or correct remaining percentage, reset time, credits balance, and notes.
- State persists across app restarts in `%APPDATA%\QuotaDock\state.json`.
- Corrupt local state is backed up and recovered without crashing.
- Low-usage and stale-data notifications respect the default cooldown rules.
- The user can enable and disable launch at startup from Settings.
- The official Usage Dashboard opens from both the main window and tray menu.
- Parser, storage, probe, and UI-state tests pass.
- A portable Windows build can be produced from a clean checkout.

## Milestones

1. Repository and design spec.
2. Tauri app scaffold with main window.
3. JSON storage and UI state shell.
4. `codex` health probe.
5. `/status` parser with fixture tests.
6. Paste/manual update flow.
7. Tray menu.
8. Notification thresholds.
9. Windows packaging.

## MVP Defaults

- Frontend framework: Svelte with TypeScript.
- Visual style: compact utility dashboard with restrained colors, no marketing-style landing page.
- Icon: simple gauge/dock mark, created during the implementation phase.
- Clipboard monitoring: deferred until after paste/manual parsing works.
- History retention: keep the latest 100 snapshots by default.
- Automatic updates: deferred until after the first local Windows package is usable.
