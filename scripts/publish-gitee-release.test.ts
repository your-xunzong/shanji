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
});
