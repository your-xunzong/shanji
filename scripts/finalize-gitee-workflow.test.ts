import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { describe, expect, it } from 'vitest';

const source = readFileSync(resolve('.github/workflows/finalize-gitee-release.yml'), 'utf8');

describe('Gitee release recovery workflow', () => {
  it('can be retried from main without moving the published tag', () => {
    expect(source).toContain('workflow_dispatch:');
    expect(source).toContain('RELEASE_TAG: ${{ github.event.release.tag_name || inputs.tag }}');
    expect(source).not.toContain('ref: ${{ github.event.release.tag_name }}');
  });
});
