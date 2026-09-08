import { readFileSync, readdirSync } from 'node:fs';
import { join } from 'node:path';
import { describe, expect, it } from 'vitest';

describe('个人仓库桌面权限', () => {
  it('仅主窗口可打开原生文件夹选择器', () => {
    const directory = join(process.cwd(), 'src-tauri', 'capabilities');
    const capabilities = readdirSync(directory).filter((name) => name.endsWith('.json')).map((name) => (
      JSON.parse(readFileSync(join(directory, name), 'utf8')) as { windows: string[]; permissions: string[] }
    ));
    const allowedWindows = capabilities.filter((capability) => capability.permissions.includes('dialog:allow-open'))
      .flatMap((capability) => capability.windows);
    expect([...new Set(allowedWindows)]).toEqual(['main']);
  });
});
