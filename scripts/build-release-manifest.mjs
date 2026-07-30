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
const encodedFilename = filename
  .split("/")
  .map((part) => encodeURIComponent(part))
  .join("/");
const url = `https://github.com/${repository}/releases/download/v${version}/${encodedFilename}`;

const manifest = {
  version,
  notes: `QuotaDock v${version}：结构化额度查询、签名更新、单实例、详情设置与轻量趋势。`,
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
