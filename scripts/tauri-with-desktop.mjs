import { spawnSync } from "node:child_process";
import { existsSync } from "node:fs";
import { join } from "node:path";

const args = process.argv.slice(2);
const [command, ...rest] = args;
const tauriArgs =
  command === "build" || command === "dev"
    ? [command, "--features", "desktop", ...rest]
    : args;

const runner = join(
  process.cwd(),
  "node_modules",
  ".bin",
  process.platform === "win32" ? "tauri.cmd" : "tauri",
);

if (!existsSync(runner)) {
  console.error("Missing local Tauri CLI. Run npm install before invoking tauri.");
  process.exit(1);
}

const result = spawnSync(runner, tauriArgs, {
  stdio: "inherit",
  shell: process.platform === "win32",
});

if (result.error) {
  console.error(result.error.message);
}

process.exit(result.status ?? 1);
