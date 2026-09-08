import { mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { basename, dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

function argumentsMap(values) {
  const result = new Map();
  for (let index = 0; index < values.length; index += 2) {
    const key = values[index];
    const value = values[index + 1];
    if (!key?.startsWith('--') || !value) throw new Error(`Invalid argument near ${key ?? 'end of command'}.`);
    result.set(key.slice(2), value);
  }
  return result;
}

function safeVersion(value) {
  if (!/^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?$/.test(value)) throw new Error(`Invalid release version: ${value}`);
  return value;
}

function safeRepository(value) {
  if (!/^[0-9A-Za-z_.-]+\/[0-9A-Za-z_.-]+$/.test(value)) throw new Error(`Invalid GitHub repository: ${value}`);
  return value;
}

function signature(path) {
  const value = readFileSync(path, 'utf8').trim();
  if (!value) throw new Error(`Empty updater signature: ${path}`);
  return value;
}

function assetUrl(repository, tag, path) {
  return `https://github.com/${repository}/releases/download/${encodeURIComponent(tag)}/${encodeURIComponent(basename(path))}`;
}

export function createManifest({ assetsDirectory, version, repository, notesPath, publishedAt }) {
  const checkedVersion = safeVersion(version);
  const checkedRepository = safeRepository(repository);
  const tag = `v${checkedVersion}`;
  const asset = (name) => join(assetsDirectory, name);
  const windows = asset(`shanji-${checkedVersion}-windows-x64-setup.exe`);
  const linux = asset(`shanji-${checkedVersion}-linux-x64.AppImage`);
  const macos = asset(`shanji-${checkedVersion}-macos-universal.app.tar.gz`);
  const notes = readFileSync(notesPath, 'utf8').trim();
  if (!notes) throw new Error(`Release notes are empty: ${notesPath}`);

  const platform = (path) => ({
    signature: signature(`${path}.sig`),
    url: assetUrl(checkedRepository, tag, path),
  });
  const macosPlatform = platform(macos);
  return {
    version: checkedVersion,
    notes,
    pub_date: publishedAt,
    platforms: {
      'windows-x86_64': platform(windows),
      'linux-x86_64': platform(linux),
      'darwin-x86_64': macosPlatform,
      'darwin-aarch64': macosPlatform,
    },
  };
}

function main() {
  const args = argumentsMap(process.argv.slice(2));
  const assetsDirectory = resolve(args.get('assets') ?? 'release-assets');
  const output = resolve(args.get('output') ?? join(assetsDirectory, 'latest.json'));
  const manifest = createManifest({
    assetsDirectory,
    version: args.get('version'),
    repository: args.get('repo'),
    notesPath: resolve(args.get('notes')),
    publishedAt: args.get('published-at') ?? new Date().toISOString(),
  });
  mkdirSync(dirname(output), { recursive: true });
  writeFileSync(output, `${JSON.stringify(manifest, null, 2)}\n`, 'utf8');
  console.log(`Created signed updater manifest: ${output}`);
}

if (process.argv[1] && fileURLToPath(import.meta.url) === resolve(process.argv[1])) main();
