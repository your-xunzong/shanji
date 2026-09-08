import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { afterEach, describe, expect, it, vi } from 'vitest';
import EmailRulesPanel from './EmailRulesPanel.svelte';
import { api } from '../lib/api';
import type { Settings, SmtpStatus } from '../lib/types';

const settings = {
  smtpTo: 'me@example.com',
} as Settings;

afterEach(() => vi.restoreAllMocks());

describe('EmailRulesPanel', () => {
  it('在发送连接未验证时阻止启用规则，并保留可停用保存的路径', async () => {
    vi.spyOn(api, 'listEmailDeliveryRules').mockResolvedValue([]);
    render(EmailRulesPanel, {
      categories: [],
      tags: [],
      settings,
      smtpStatus: { passwordConfigured: true, verifiedAt: null, lastResult: null, lastErrorCode: null, lastAttemptAt: null } satisfies SmtpStatus,
      onClose: vi.fn(),
      onOpenSettings: vi.fn(),
    });

    await screen.findByText('还没有邮件提醒规则');
    await fireEvent.click(screen.getByRole('button', { name: '建立第一条规则' }));
    expect(screen.getByRole('checkbox', { name: /启用这条规则/ })).toBeDisabled();
    expect(screen.getByText('需要先通过发送测试')).toBeInTheDocument();
  });

  it('保存经过验证的规则并显示投递路径', async () => {
    vi.spyOn(api, 'listEmailDeliveryRules').mockResolvedValue([]);
    const save = vi.spyOn(api, 'saveEmailDeliveryRule').mockImplementation(async (input) => ({
      ...input,
      id: 'rule-1',
      createdAt: '2026-09-03T08:00:00Z',
      updatedAt: '2026-09-03T08:00:00Z',
    }));
    render(EmailRulesPanel, {
      categories: [],
      tags: [],
      settings,
      smtpStatus: { passwordConfigured: true, verifiedAt: '2026-09-03T08:00:00Z', lastResult: 'SUBMITTED', lastErrorCode: null, lastAttemptAt: '2026-09-03T08:00:00Z' } satisfies SmtpStatus,
      onClose: vi.fn(),
      onOpenSettings: vi.fn(),
    });

    await screen.findByText('还没有邮件提醒规则');
    await fireEvent.click(screen.getByRole('button', { name: '建立第一条规则' }));
    await fireEvent.click(screen.getByRole('button', { name: '保存规则' }));
    await waitFor(() => expect(save).toHaveBeenCalledWith(expect.objectContaining({ enabled: true, recipient: 'me@example.com' })));
    expect(screen.getByText('邮件提醒规则已保存。')).toBeInTheDocument();
    expect(screen.getByText('首次到期时')).toBeInTheDocument();
  });

  it('读取失败时不显示后端技术信息', async () => {
    vi.spyOn(api, 'listEmailDeliveryRules').mockRejectedValue(new Error('SQLITE_BUSY: internal-rule-table'));
    render(EmailRulesPanel, {
      categories: [],
      tags: [],
      settings,
      smtpStatus: { passwordConfigured: false, verifiedAt: null, lastResult: null, lastErrorCode: null, lastAttemptAt: null } satisfies SmtpStatus,
      onClose: vi.fn(),
      onOpenSettings: vi.fn(),
    });

    expect(await screen.findByText('邮件提醒规则暂时无法读取，请稍后重试。')).toBeInTheDocument();
    expect(screen.queryByText(/SQLITE_BUSY/)).not.toBeInTheDocument();
  });
});
