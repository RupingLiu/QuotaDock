import { copyFileSync, existsSync, mkdirSync, statSync } from "node:fs";
import { join, resolve } from "node:path";

function findProjectRoot() {
  const cwd = process.cwd();
  if (existsSync(join(cwd, "src-tauri", "tauri.conf.json"))) {
    return cwd;
  }
  if (existsSync(join(cwd, "tauri.conf.json"))) {
    return resolve(cwd, "..");
  }
  throw new Error(`Cannot locate QuotaDock project root from ${cwd}`);
}

const projectRoot = findProjectRoot();
const targetDir = process.env.CARGO_TARGET_DIR
  ? resolve(projectRoot, process.env.CARGO_TARGET_DIR)
  : join(projectRoot, "src-tauri", "target");

const candidates = [
  join(targetDir, "release", "WebView2Loader.dll"),
  join(targetDir, "debug", "WebView2Loader.dll"),
];

const source = candidates.find((candidate) => existsSync(candidate));

if (!source) {
  throw new Error(
    `WebView2Loader.dll was not found. Checked: ${candidates.join(", ")}`,
  );
}

const destinationDir = join(projectRoot, "src-tauri", "resources", "windows");
const destination = join(destinationDir, "WebView2Loader.dll");

mkdirSync(destinationDir, { recursive: true });
copyFileSync(source, destination);

const size = statSync(destination).size;
console.log(`Synced WebView2Loader.dll (${size} bytes) to ${destination}`);
