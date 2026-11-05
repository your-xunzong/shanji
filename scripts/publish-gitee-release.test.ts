import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { describe, expect, it } from 'vitest';

const source = readFileSync(resolve('scripts/publish-gitee-release.mjs'), 'utf8');

describe('Gitee release publisher contract', () => {
  it('sends the required target branch and keeps the token out of request URLs', () => {
    expect(source).toContain("form.set('target_commitish', 'main')");
    expect(source).toContain("headers.set('Authorization', `token ${token}`)");
    expect(source).not.toContain("url.searchParams.set('access_token'");
  });

  it('uses a long-running curl upload with the token supplied through stdin', () => {
    expect(source).toContain("'max-time = 1800'");
    expect(source).toContain('`form = "access_token=${token}"`');
    expect(source).toContain("['--config', '-', '--request', 'POST'");
    expect(source).toContain('.sort((left, right) => statSync(left).size - statSync(right).size)');
  });

  it('uses the dedicated attachment endpoint for idempotent replacement and verification', () => {
    expect(source).toContain('async function listReleaseAssets');
    expect(source).toContain('/attach_files`, token');
    expect(source).toContain('for (const asset of prior)');
    expect(source).toContain('/attach_files/${asset.id}`');
    expect(source).not.toContain('/attach_files/${prior.id}`');
  });

  it('retries transient Gitee API failures without retrying the asset upload command', () => {
    expect(source).toContain('const REQUEST_ATTEMPTS = 5');
    expect(source).toContain('Gitee API connection failed; retrying');
    expect(source).toContain('[408, 429, 500, 502, 503, 504]');
    expect(source).not.toContain("'retry-all-errors'");
  });

  it('reuses a complete attachment so a failed release can resume', () => {
    expect(source).toContain('const reusable = prior.find((asset) => Number(asset.size) === size)');
    expect(source).toContain('if (asset.id === reusable?.id) continue');
    expect(source).toContain('Reusing Gitee release asset');
    expect(source).toContain('if (matching.length !== 1)');
  });

  it('uploads at bounded concurrency and validates each upload response', () => {
    expect(source).toContain('const UPLOAD_CONCURRENCY = 3');
    expect(source).toContain('function uploadAsset');
    expect(source).toContain('async function forEachConcurrent');
    expect(source).toContain('await Promise.all(workers)');
    expect(source).toContain('uploaded.name !== name');
  });
});
