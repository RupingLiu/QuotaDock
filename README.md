# QuotaDock

QuotaDock is a planned lightweight desktop utility for tracking personal Codex usage.

The first design focuses on a Windows-first Tauri app with a small main window, tray access, and semi-automatic usage updates from pasted Codex `/status` output. It avoids relying on unofficial personal subscription APIs.

See the current design spec:

- [Codex usage tool design](docs/superpowers/specs/2026-06-18-codex-usage-tool-design.md)

## Development

Install dependencies:

```powershell
npm install
```

Run the Svelte development server:

```powershell
npm run dev
```

Run checks:

```powershell
npm run test
```

Run the desktop app in development:

```powershell
npm run tauri dev
```

Build the desktop app:

```powershell
npm run tauri build
```
