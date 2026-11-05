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
});
