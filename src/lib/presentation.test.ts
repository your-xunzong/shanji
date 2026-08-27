import { describe, expect, it, vi } from 'vitest';
import {
  duePreview,
  itemTiming,
  mustCompletePreview,
  nextReminderLabel,
  toLocalDateTimeInput,
} from './presentation';
import type { Item } from './types';

const baseItem: Item = {
  id: 'item-1',
  title: '提交版本说明',
  notes: '',
  status: 'OPEN',
  categoryId: null,
  categoryName: '收件箱',
  dueAt: '2026-08-27T10:00:00.000Z',
  dueLocalDate: '2026-08-27',
  dueLocalTime: '18:00',
  dueSource: 'DEFAULT_EOD',
  rolloverPolicy: 'NEXT_WORKDAY_EOD',
  rolloverCount: 0,
  completionPolicy: 'NORMAL',
  repeatIntervalMinutes: null,
  nextReminderAt: '2026-08-27T10:00:00.000Z',
  reminderPaused: false,
  bypassAppQuietHours: false,
  createdAt: '2026-08-27T01:00:00.000Z',
  updatedAt: '2026-08-27T01:00:00.000Z',
  completedAt: null,
};

describe('itemTiming', () => {
  it('今日必做逾期时同时显示紧急语义', () => {
    const result = itemTiming(
      { ...baseItem, completionPolicy: 'MUST_COMPLETE_TODAY' },
      new Date('2026-08-27T11:00:00.000Z'),
    );
    expect(result.tone).toBe('overdue');
    expect(result.label).toContain('今日必做');
    expect(result.label).toContain('已逾期');
  });

  it('完成事项不再显示到期状态', () => {
    expect(itemTiming({ ...baseItem, status: 'DONE' }).label).toBe('已完成');
  });

  it('时间标签优先展示后端保存的本地时间意图', () => {
    const result = itemTiming(
      { ...baseItem, dueAt: '2026-08-27T12:00:00.000Z', dueLocalTime: '15:00' },
      new Date('2026-08-27T11:00:00.000Z'),
    );
    expect(result.label).toContain('15:00');
  });
});

describe('duePreview', () => {
  it('下班后创建显示下一工作日', () => {
    vi.stubGlobal(
      'Intl',
      globalThis.Intl,
    );
    expect(duePreview('18:00', [1, 2, 3, 4, 5], new Date(2026, 7, 27, 19, 0))).toContain('下一工作日');
  });

  it('非工作日即使早于默认时间也显示下一工作日', () => {
    expect(duePreview('18:00', [1, 2, 3, 4, 5], new Date(2026, 7, 29, 10, 0))).toContain('下一工作日');
  });
});

describe('今日必做时间预览', () => {
  it('超过默认时间后明确提示立即提醒', () => {
    expect(mustCompletePreview('15:00', new Date(2026, 7, 27, 22, 0))).toBe(
      '今日必做：保存后立即提醒',
    );
  });

  it('默认时间未到时仍安排在今天', () => {
    expect(mustCompletePreview('15:00', new Date(2026, 7, 27, 10, 0))).toBe(
      '今日必做：今天 15:00',
    );
  });
});

describe('时间调整展示', () => {
  it('跨日的下一次提醒会显示日期', () => {
    const label = nextReminderLabel(
      { ...baseItem, nextReminderAt: '2026-08-28T01:30:00.000Z' },
      new Date('2026-08-27T12:00:00.000Z'),
    );
    expect(label).toContain('8月28日');
  });

  it('编辑器初值使用本地日期时间格式', () => {
    expect(toLocalDateTimeInput(new Date(2026, 7, 28, 9, 30).toISOString())).toBe(
      '2026-08-28T09:30',
    );
  });
});
