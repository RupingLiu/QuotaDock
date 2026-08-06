import { createHash } from "node:crypto";
import { readFile, stat, writeFile } from "node:fs/promises";
import { basename, dirname, resolve } from "node:path";

const [version, installerArg, signatureArg, repository = "RupingLiu/QuotaDock"] =
  process.argv.slice(2);

if (!version || !/^\d+\.\d+\.\d+(?:[-+][0-9A-Za-z.-]+)?$/.test(version)) {
  throw new Error("Usage: node scripts/build-release-manifest.mjs <version> <installer> <signature> [owner/repo]");
}

const installer = resolve(installerArg ?? "");
const signatureFile = resolve(signatureArg ?? "");
const filename = basename(installer);
const outputDirectory = dirname(installer);
const bytes = await readFile(installer);
const signature = (await readFile(signatureFile, "utf8")).trim();
const { size } = await stat(installer);
const sha256 = createHash("sha256").update(bytes).digest("hex");
const releaseNotes = await readReleaseNotes(version);
const encodedFilename = filename
  .split("/")
  .map((part) => encodeURIComponent(part))
  .join("/");
const url = `https://github.com/${repository}/releases/download/v${version}/${encodedFilename}`;

const manifest = {
  version,
  notes: releaseNotes,
  pub_date: new Date().toISOString(),
  platforms: {
    "windows-x86_64": {
      signature,
      url,
      // Kept for the v0.2.x updater so existing users can cross the trust-chain migration.
      sha256,
      size,
      filename,
    },
  },
};

await Promise.all([
  writeFile(
    resolve(outputDirectory, "latest.json"),
    `${JSON.stringify(manifest, null, 2)}\n`,
    "utf8",
  ),
  writeFile(
    resolve(outputDirectory, `${filename}.sha256`),
    `${sha256}  ${filename}\n`,
    "utf8",
  ),
]);

console.log(JSON.stringify({ version, filename, size, sha256 }, null, 2));

async function readReleaseNotes(releaseVersion) {
  try {
    const markdown = await readFile(
      resolve(`docs/releases/v${releaseVersion}.md`),
      "utf8",
    );
    const highlights = markdown
      .split(/\r?\n/)
      .map((line) => line.trim())
      .filter((line) => line.startsWith("- "))
      .slice(0, 4)
      .map((line) => `• ${line.slice(2).replaceAll("`", "")}`);
    if (highlights.length > 0) {
      return highlights.join("\n");
    }
  } catch (error) {
    if (error?.code !== "ENOENT") throw error;
  }
  return `QuotaDock v${releaseVersion}：签名更新与稳定性改进。`;
}
