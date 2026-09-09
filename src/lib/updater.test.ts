import { describe, expect, it } from 'vitest';
import { isTrustedReleaseJson, parseUserFacingNotes } from './updater';

describe('updater metadata', () => {
  it('extracts at most six user-facing bullet points', () => {
    const notes = parseUserFacingNotes([
      '# v1.2.0',
      '- 第一项', '- 第二项', '- 第三项', '- 第四项', '- 第五项', '- 第六项', '- 第七项',
    ].join('\n'));

    expect(notes).toEqual(['第一项', '第二项', '第三项', '第四项', '第五项', '第六项']);
  });

  it('keeps remote markup as escaped plain text instead of rendering HTML', () => {
    expect(parseUserFacingNotes('- <script>不能执行</script>')).toEqual(['<script>不能执行</script>']);
  });

  it('accepts only assets from the official repository release path', () => {
    expect(isTrustedReleaseJson({
      platforms: {
        'windows-x86_64': {
          url: 'https://github.com/your-xunzong/shanji/releases/download/v0.12.1/shanji.exe',
        },
      },
    })).toBe(true);
    expect(isTrustedReleaseJson({
      platforms: { 'windows-x86_64': { url: 'https://downloads.example.com/shanji.exe' } },
    })).toBe(false);
    expect(isTrustedReleaseJson({ version: '0.12.1' })).toBe(false);
  });
});
