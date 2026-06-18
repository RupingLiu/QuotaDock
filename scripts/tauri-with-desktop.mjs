import { spawnSync } from "node:child_process";
import { existsSync } from "node:fs";
import { join } from "node:path";

const args = process.argv.slice(2);
const [command, ...rest] = args;
const tauriArgs =
  command === "build" || command === "dev"
    ? [command, "--features", "desktop", ...rest]
    : args;

const tauriEntry = join(
  process.cwd(),
  "node_modules",
  "@tauri-apps",
  "cli",
  "tauri.js",
);

if (!existsSync(tauriEntry)) {
  console.error("Missing local Tauri CLI. Run npm install before invoking tauri.");
  process.exit(1);
}

const result = spawnSync(process.execPath, [tauriEntry, ...tauriArgs], {
  stdio: "inherit",
  shell: false,
});

if (result.error) {
  console.error(result.error.message);
}

process.exit(result.status ?? 1);
