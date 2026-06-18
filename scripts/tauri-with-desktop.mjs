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

const env = { ...process.env };
const pathKey =
  Object.keys(env).find((key) => key.toLowerCase() === "path") ?? "PATH";
for (const candidate of [
  "C:\\msys64\\ucrt64\\bin",
  "C:\\msys64\\mingw64\\bin",
  "C:\\msys64\\clang64\\bin",
]) {
  if (existsSync(join(candidate, "dlltool.exe"))) {
    env[pathKey] = `${candidate};${env[pathKey] ?? ""}`;
    break;
  }
}

const result = spawnSync(process.execPath, [tauriEntry, ...tauriArgs], {
  stdio: "inherit",
  shell: false,
  env,
});

if (result.error) {
  console.error(result.error.message);
}

process.exit(result.status ?? 1);
