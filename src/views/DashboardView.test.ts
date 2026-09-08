import { fireEvent, render, screen } from '@testing-library/svelte';
import { afterEach, describe, expect, it, vi } from 'vitest';
import DashboardView from './DashboardView.svelte';
import { api } from '../lib/api';

afterEach(() => vi.restoreAllMocks());

describe('工作台读取恢复', () => {
  it('读取失败不暴露内部错误，重试后回到事项列表', async () => {
    vi.spyOn(api, 'getOnboardingStatus').mockResolvedValue({ required: false, completedVersion: 3, currentVersion: 3 });
    const read = vi.spyOn(api, 'listItems').mockRejectedValueOnce(new Error('IPC SQL state not managed')).mockResolvedValue([]);
    render(DashboardView);
    expect(await screen.findByText('事项暂时无法读取。闪记没有修改记录，请重试。')).toBeInTheDocument();
    expect(screen.queryByText(/IPC|SQL|state not managed/)).not.toBeInTheDocument();
    expect(screen.queryByText('这里暂时是空的')).not.toBeInTheDocument();
    await fireEvent.click(screen.getByRole('button', { name: '重试' }));
    expect(await screen.findByText('这里暂时是空的')).toBeInTheDocument();
    expect(read.mock.calls.length).toBeGreaterThan(1);
    expect(screen.queryByRole('alert')).not.toBeInTheDocument();
  });
});
