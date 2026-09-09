import { existsSync, readFileSync } from "node:fs";

function readJson(path) {
  return JSON.parse(readFileSync(path, "utf8"));
}

function packageVersionFromToml(path) {
  const content = readFileSync(path, "utf8");
  const packageHeader = "[package]";
  const packageStart = content.indexOf(packageHeader);
  if (packageStart < 0) {
    throw new Error(`Cannot find [package] in ${path}.`);
  }
  const afterHeader = content.slice(packageStart + packageHeader.length);
  const nextSection = afterHeader.search(/^\[/m);
  const packageBlock = nextSection < 0 ? afterHeader : afterHeader.slice(0, nextSection);
  const version = packageBlock?.match(/^version\s*=\s*"([^"]+)"\s*$/m)?.[1];
  if (!version) {
    throw new Error(`Cannot find package version in ${path}.`);
  }
  return version;
}

function packageVersionFromLock(path, packageName) {
  const content = readFileSync(path, "utf8");
  const packageBlock = content
    .split("[[package]]")
    .find((block) => new RegExp(`^name\\s*=\\s*"${packageName}"\\s*$`, "m").test(block));
  const version = packageBlock?.match(/^version\s*=\s*"([^"]+)"\s*$/m)?.[1];
  if (!version) {
    throw new Error(`Cannot find ${packageName} version in ${path}.`);
  }
  return version;
}

const versions = new Map([
  ["package.json", readJson("package.json").version],
  ["src-tauri/Cargo.toml", packageVersionFromToml("src-tauri/Cargo.toml")],
  ["src-tauri/Cargo.lock", packageVersionFromLock("src-tauri/Cargo.lock", "shanji")],
  ["src-tauri/tauri.conf.json", readJson("src-tauri/tauri.conf.json").version],
]);

const uniqueVersions = new Set(versions.values());
if (uniqueVersions.size !== 1) {
  const details = [...versions].map(([path, version]) => `${path}: ${version}`).join("\n");
  throw new Error(`Application versions do not match:\n${details}`);
}

const [version] = uniqueVersions;
const releaseNotesPath = `src/release-notes/v${version}.md`;
if (!existsSync(releaseNotesPath)) {
  throw new Error(`Missing user-facing release notes: ${releaseNotesPath}`);
}
const releaseNotes = readFileSync(releaseNotesPath, "utf8");
const releaseBullets = releaseNotes.match(/^\s*[-*]\s+\S.+$/gm) ?? [];
if (releaseBullets.length < 3 || releaseBullets.length > 6) {
  throw new Error(`${releaseNotesPath} must contain 3-6 user-facing bullet points.`);
}

const tauriConfig = readJson("src-tauri/tauri.conf.json");
const updaterConfig = tauriConfig.plugins?.updater;
if (tauriConfig.bundle?.createUpdaterArtifacts !== true) {
  throw new Error("Tauri updater artifacts are not enabled.");
}
if (!updaterConfig?.pubkey || !Array.isArray(updaterConfig.endpoints)
  || updaterConfig.endpoints.length !== 1
  || updaterConfig.endpoints[0] !== "https://github.com/your-xunzong/shanji/releases/latest/download/latest.json") {
  throw new Error("Tauri updater must use the signed official GitHub release endpoint.");
}
if (process.env.GITHUB_REF_TYPE === "tag") {
  const expectedTag = `v${version}`;
  if (process.env.GITHUB_REF_NAME !== expectedTag) {
    throw new Error(
      `Release tag ${process.env.GITHUB_REF_NAME} does not match application version ${expectedTag}.`,
    );
  }
}

console.log(`Application version ${version} is consistent.`);
console.log(`Release notes contain ${releaseBullets.length} user-facing changes.`);
