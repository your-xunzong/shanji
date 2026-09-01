import { fireEvent, render, screen } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';
import type { Item } from '../lib/types';
import ItemCard from './ItemCard.svelte';

const item: Item = {
  id: 'item-1', title: '整理需求', notes: '', status: 'OPEN',
  categoryId: 'work', categoryName: '工作',
  dueAt: '2026-08-28T10:00:00.000Z', dueLocalDate: '2026-08-28', dueLocalTime: '18:00',
  dueSource: 'EXPLICIT', rolloverPolicy: 'NONE', rolloverCount: 0,
  completionPolicy: 'NORMAL', repeatIntervalMinutes: null,
  nextReminderAt: '2026-08-28T10:00:00.000Z', reminderPaused: false,
  bypassAppQuietHours: false, createdAt: '2026-08-28T09:00:00.000Z',
  updatedAt: '2026-08-28T09:00:00.000Z', completedAt: null, deletedAt: null,
  eventKind: 'ORDINARY', reminderPlan: 'REPEAT', important: false, timeMode: 'SPECIFIED',
  startAt: null, endAt: null, targetAt: null, leadValue: null, leadUnit: null,
  cadenceValue: null, cadenceUnit: null, emphasisMaxPerDay: 8,
  tags: [{ id: 'customer', name: '客户', color: '#B06C49' }],
};

describe('ItemCard', () => {
  it('opens a complete item editor with type and tag choices', async () => {
    render(ItemCard, { props: {
      item,
      categories: [{ id: 'work', name: '工作', color: '#627D98' }],
      tags: item.tags,
      onComplete: vi.fn(), onPause: vi.fn(), onReschedule: vi.fn(),
      onUpdate: vi.fn(), onDelete: vi.fn(), onPermanentDelete: vi.fn(),
    } });
    await fireEvent.click(screen.getByRole('button', { name: '编辑' }));
    expect(screen.getByText('事项内容')).toBeInTheDocument();
    expect(screen.getByLabelText('事件类型')).toHaveValue('ORDINARY');
    expect(screen.getByLabelText('类型')).toHaveValue('work');
    expect(screen.getByRole('button', { name: '客户' })).toHaveClass('active');
    expect(screen.getByRole('button', { name: '保存修改' })).toBeInTheDocument();
  });
});
