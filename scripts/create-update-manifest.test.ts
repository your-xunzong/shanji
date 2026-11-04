import { mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { tmpdir } from 'node:os';
import { afterEach, describe, expect, it } from 'vitest';
// The production script remains directly executable by Node and intentionally has no TS wrapper.
// @ts-expect-error JavaScript module is covered by this runtime contract test.
import { createManifest } from './create-update-manifest.mjs';

let directory = '';

afterEach(() => {
  if (directory) rmSync(directory, { recursive: true, force: true });
  directory = '';
});

describe('dual-source updater manifest', () => {
  it('uses the selected repository host while preserving identical signatures', () => {
    directory = mkdtempSync(join(tmpdir(), 'shanji-manifest-'));
    const version = '0.14.0';
    for (const name of [
      `shanji-${version}-windows-x64-setup.exe`,
      `shanji-${version}-linux-x64.AppImage`,
      `shanji-${version}-macos-universal.app.tar.gz`,
    ]) {
      writeFileSync(join(directory, `${name}.sig`), `signature-${name}`);
    }
    const notesPath = join(directory, 'notes.md');
    writeFileSync(notesPath, '- 双源更新');

    const manifest = createManifest({
      assetsDirectory: directory,
      version,
      repository: 'wu_-ji/shanji',
      notesPath,
      publishedAt: '2026-11-04T00:00:00Z',
      provider: 'gitee',
    });

    expect(manifest.platforms['windows-x86_64'].url).toBe(
      'https://gitee.com/wu_-ji/shanji/releases/download/v0.14.0/shanji-0.14.0-windows-x64-setup.exe',
    );
    expect(manifest.platforms['darwin-aarch64']).toEqual(manifest.platforms['darwin-x86_64']);
    expect(manifest.platforms['linux-x86_64'].signature).toContain('linux-x64.AppImage');
  });
});
