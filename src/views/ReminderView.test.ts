import { fireEvent, render, screen } from '@testing-library/svelte';
import { afterEach, describe, expect, it, vi } from 'vitest';
import ReminderView from './ReminderView.svelte';
import { api } from '../lib/api';

afterEach(() => {
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

describe('ReminderView', () => {
  it('keeps the reminder and close action when hiding fails', async () => {
    vi.spyOn(api, 'hideReminder').mockRejectedValueOnce(new Error('window hide failed'));
    render(ReminderView);

    expect(await screen.findByText('下班前提交发布审批')).toBeInTheDocument();
    await fireEvent.click(screen.getByRole('button', { name: '关闭本次提醒窗' }));

    expect(await screen.findByRole('alert')).toHaveTextContent('提醒窗没有关闭');
    expect(screen.getByText('下班前提交发布审批')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: '关闭本次提醒窗' })).toBeEnabled();
  });

  it('hides before removing the current reminder and leaves an empty-state close action', async () => {
    const hide = vi.spyOn(api, 'hideReminder').mockResolvedValue(undefined);
    render(ReminderView);

    await fireEvent.click(await screen.findByRole('button', { name: '关闭本次提醒窗' }));
    expect(hide).toHaveBeenCalledTimes(1);
    expect(screen.queryByText('下班前提交发布审批')).not.toBeInTheDocument();
    expect(screen.getByText(/提醒内容正在载入/)).toBeInTheDocument();
    expect(screen.getByRole('button', { name: '关闭本次提醒窗' })).toBeInTheDocument();
  });

  it('ignores composing Escape and closes on a normal Escape', async () => {
    const hide = vi.spyOn(api, 'hideReminder').mockResolvedValue(undefined);
    render(ReminderView);
    await screen.findByText('下班前提交发布审批');

    await fireEvent.keyDown(window, { key: 'Escape', isComposing: true });
    expect(hide).not.toHaveBeenCalled();
    await fireEvent.keyDown(window, { key: 'Escape', isComposing: false });
    expect(hide).toHaveBeenCalledTimes(1);
  });
});
