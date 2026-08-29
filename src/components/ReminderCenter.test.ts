import { fireEvent, render, screen } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';
import type { Item } from '../lib/types';
import ReminderCenter from './ReminderCenter.svelte';

const item: Item = {
  id: 'pending-1', title: '确认发布内容', notes: '', status: 'OPEN',
  categoryId: 'work', categoryName: '工作',
  dueAt: '2026-08-29T10:00:00.000Z', dueLocalDate: '2026-08-29', dueLocalTime: '18:00',
  dueSource: 'EXPLICIT', rolloverPolicy: 'NONE', rolloverCount: 0,
  completionPolicy: 'MUST_COMPLETE_TODAY', repeatIntervalMinutes: 30,
  nextReminderAt: '2026-08-29T10:30:00.000Z', reminderPaused: false,
  bypassAppQuietHours: true, createdAt: '2026-08-29T09:00:00.000Z',
  updatedAt: '2026-08-29T10:00:00.000Z', completedAt: null, deletedAt: null,
  tags: [],
};

describe('ReminderCenter', () => {
  it('keeps all reminder decisions explicit', async () => {
    const onComplete = vi.fn().mockResolvedValue(undefined);
    const onSnooze = vi.fn().mockResolvedValue(undefined);
    const onAcknowledge = vi.fn().mockResolvedValue(undefined);
    const onOpen = vi.fn().mockResolvedValue(undefined);
    render(ReminderCenter, {
      props: {
        items: [item], onClose: vi.fn(), onComplete, onSnooze, onAcknowledge, onOpen,
      },
    });

    expect(screen.getByText('确认发布内容')).toBeInTheDocument();
    await fireEvent.click(screen.getByRole('button', { name: '15 分钟后提醒' }));
    expect(onSnooze).toHaveBeenCalledWith(item, 15);
    await fireEvent.click(screen.getByRole('button', { name: '确认已看到' }));
    expect(onAcknowledge).toHaveBeenCalledWith(item);
    await fireEvent.click(screen.getByRole('button', { name: '完成' }));
    expect(onComplete).toHaveBeenCalledWith(item);
    await fireEvent.click(screen.getByRole('button', { name: '打开' }));
    expect(onOpen).toHaveBeenCalledWith(item);
  });
});
