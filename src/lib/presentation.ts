import type { EventKind, Item, ReminderPlan, ScheduleUnit } from './types';

export const EVENT_KIND_OPTIONS: { value: EventKind; label: string; description: string }[] = [
  { value: 'ORDINARY', label: '普通', description: '每天在固定时间提醒' },
  { value: 'ONE_TIME', label: '一次性', description: '提醒一次后停止' },
  { value: 'TODAY_MUST', label: '今日必做', description: '完成前持续提醒' },
  { value: 'WARNING', label: '预警', description: '目标发生前每天提醒' },
  { value: 'CONTINUOUS', label: '持续', description: '在一段时间内按周期提醒' },
  { value: 'MONTHLY', label: '月度', description: '每月生成下一次提醒' },
  { value: 'YEARLY', label: '年度', description: '每年生成下一次提醒' },
];

export const REMINDER_PLAN_OPTIONS: { value: ReminderPlan; label: string }[] = [
  { value: 'REPEAT', label: '重复' },
  { value: 'EMPHASIS', label: '强调' },
  { value: 'ONCE', label: '一次性' },
  { value: 'FORCE', label: '强制' },
  { value: 'CUSTOM', label: '自定义' },
];

export const SCHEDULE_UNIT_OPTIONS: { value: ScheduleUnit; label: string }[] = [
  { value: 'DAY', label: '天' },
  { value: 'WEEK', label: '周' },
  { value: 'MONTH', label: '月' },
  { value: 'YEAR', label: '年' },
];

export function eventKindLabel(kind: EventKind | null): string {
  return EVENT_KIND_OPTIONS.find((entry) => entry.value === kind)?.label ?? '待选择事件类型';
}

export function reminderPlanLabel(plan: ReminderPlan): string {
  return REMINDER_PLAN_OPTIONS.find((entry) => entry.value === plan)?.label ?? plan;
}

export const localDateKey = (value: Date): string =>
  `${value.getFullYear()}-${String(value.getMonth() + 1).padStart(2, '0')}-${String(value.getDate()).padStart(2, '0')}`;

export function localInputToIso(value: string): string | null {
  if (!value) return null;
  const date = new Date(value);
  return Number.isNaN(date.getTime()) ? null : date.toISOString();
}

export function toLocalDateTimeInput(value: string): string {
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return '';
  return `${localDateKey(date)}T${String(date.getHours()).padStart(2, '0')}:${String(date.getMinutes()).padStart(2, '0')}`;
}

export function localDateTimeInputLabel(value: string): string {
  const match = /^(\d{4})-(\d{2})-(\d{2})T(\d{2}):(\d{2})/.exec(value);
  if (!match) return value;
  return `${Number(match[2])}月${Number(match[3])}日 ${match[4]}:${match[5]}`;
}

export function itemTiming(item: Item, now = new Date()): {
  label: string;
  tone: 'normal' | 'urgent' | 'overdue' | 'done';
} {
  if (item.status === 'DONE') {
    return { label: '已完成', tone: 'done' };
  }

  const due = new Date(item.dueAt);
  const time = item.dueLocalTime;

  if (due.getTime() < now.getTime()) {
    const prefix = item.completionPolicy === 'MUST_COMPLETE_TODAY' ? '今日必做 · ' : '';
    return { label: `${prefix}已逾期 ${time}`, tone: 'overdue' };
  }

  if (item.completionPolicy === 'MUST_COMPLETE_TODAY') {
    return { label: `今日必做 · ${time}`, tone: 'urgent' };
  }

  if (item.dueLocalDate === localDateKey(now)) {
    return { label: `今天 ${time}`, tone: 'normal' };
  }

  const [year, month, day] = item.dueLocalDate.split('-').map(Number);
  const localDate = new Date(year, month - 1, day);
  const date = new Intl.DateTimeFormat('zh-CN', {
    month: 'numeric',
    day: 'numeric',
    weekday: 'short',
  }).format(localDate);
  return { label: `${date} ${time}`, tone: 'normal' };
}

export function nextReminderLabel(item: Item, now = new Date()): string | null {
  if (item.reminderPaused) return '提醒已暂停';
  if (!item.nextReminderAt) return null;

  const next = new Date(item.nextReminderAt);
  const time = new Intl.DateTimeFormat('zh-CN', {
    hour: '2-digit',
    minute: '2-digit',
    hour12: false,
  }).format(next);
  if (localDateKey(next) === localDateKey(now)) return `下次提醒 ${time}`;
  const date = `${next.getMonth() + 1}月${next.getDate()}日`;
  return `下次提醒 ${date} ${time}`;
}

export function mustCompletePreview(settingsTime: string, now = new Date()): string {
  const [hours, minutes] = settingsTime.split(':').map(Number);
  const target = new Date(now);
  target.setHours(hours, minutes, 0, 0);
  return target.getTime() > now.getTime()
    ? `今日必做：今天 ${settingsTime}`
    : '今日必做：保存后立即提醒';
}

export function duePreview(
  settingsTime: string,
  workdays: number[] = [1, 2, 3, 4, 5],
  now = new Date(),
): string {
  const [hours, minutes] = settingsTime.split(':').map(Number);
  const due = new Date(now);
  due.setHours(hours, minutes, 0, 0);
  const jsDay = now.getDay();
  const isoDay = jsDay === 0 ? 7 : jsDay;
  const isToday = workdays.includes(isoDay) && due.getTime() > now.getTime();
  return `默认：${isToday ? '今天' : '下一工作日'} ${settingsTime}`;
}
