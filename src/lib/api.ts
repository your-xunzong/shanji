import { invoke, isTauri } from '@tauri-apps/api/core';
import type {
  Category,
  CreateItemInput,
  AutostartStatus,
  Item,
  ItemFilter,
  NotificationStatus,
  SmtpStatus,
  OnboardingStatus,
  Settings,
  UpdateSettingsInput,
  DataFileSummary,
  DataStatus,
  Tag,
  TaxonomyInput,
  UpdateItemInput,
  ExportResult,
  ExportFilterInput,
  AppInfo,
  StartupStatus,
  EventKind,
  EmailDeliveryRule,
  EmailDeliveryRuleInput,
  RepositoryPreview,
  RepositoryStatus,
  RepositorySyncResult,
  TimelineData,
  TimelineEntry,
  TimelineQuery,
  UpdateCheckResult,
  UpdateState,
} from './types';
import { localDateKey } from './presentation';

const SETTINGS_KEY = 'shanji.preview.settings';
const ITEMS_KEY = 'shanji.preview.items';
const DRAFT_KEY = 'shanji.preview.draft';
const ONBOARDING_KEY = 'shanji.preview.onboarding-version';
const CATEGORIES_KEY = 'shanji.preview.categories';
const TAGS_KEY = 'shanji.preview.tags';
const REMINDER_ACKS_KEY = 'shanji.preview.reminder-acks';
const EMAIL_ROUTES_KEY = 'shanji.preview.email-routes';
const EMAIL_RULES_KEY = 'shanji.preview.email-rules';
const SMTP_VERIFIED_KEY = 'shanji.preview.smtp-verified-at';
const REPOSITORY_KEY = 'shanji.preview.repository-path';
const REPOSITORY_SYNC_KEY = 'shanji.preview.repository-sync-at';
const UPDATE_STATE_KEY = 'shanji.preview.update-state';
const MANAGED_STATE_RETRY_DELAYS_MS = [25, 50, 100, 200, 400, 800];

interface PreviewUpdateState {
  autoCheckEnabled: boolean;
  permissionPrompted: boolean;
  lastCheckedAt: string | null;
  lastCheckResult: UpdateCheckResult | null;
  snoozedVersion: string | null;
  snoozedUntil: string | null;
  lastNotifiedVersion: string | null;
  releaseNotesSeenVersion: string | null;
}

function readPreviewUpdateState(): UpdateState {
  const defaults: PreviewUpdateState = {
    autoCheckEnabled: false,
    permissionPrompted: false,
    lastCheckedAt: null,
    lastCheckResult: null,
    snoozedVersion: null,
    snoozedUntil: null,
    lastNotifiedVersion: null,
    releaseNotesSeenVersion: '0.13.1',
  };
  const saved = localStorage.getItem(UPDATE_STATE_KEY);
  const state = saved ? { ...defaults, ...JSON.parse(saved) } : defaults;
  const lastChecked = state.lastCheckedAt ? new Date(state.lastCheckedAt).getTime() : Number.NaN;
  const shouldAutoCheck = state.autoCheckEnabled
    && (!Number.isFinite(lastChecked) || Date.now() >= lastChecked + 24 * 60 * 60 * 1000);
  return {
    ...state,
    shouldAutoCheck,
    showCurrentReleaseNotes: state.releaseNotesSeenVersion !== '0.13.1',
  };
}

function writePreviewUpdateState(state: UpdateState): UpdateState {
  localStorage.setItem(UPDATE_STATE_KEY, JSON.stringify({
    autoCheckEnabled: state.autoCheckEnabled,
    permissionPrompted: state.permissionPrompted,
    lastCheckedAt: state.lastCheckedAt,
    lastCheckResult: state.lastCheckResult,
    snoozedVersion: state.snoozedVersion,
    snoozedUntil: state.snoozedUntil,
    lastNotifiedVersion: state.lastNotifiedVersion,
    releaseNotesSeenVersion: state.releaseNotesSeenVersion,
  }));
  return readPreviewUpdateState();
}

const defaultSettings: Settings = {
  defaultDueTime: '18:00',
  repeatDefaultTimes: ['10:00', '17:00'],
  workdays: [1, 2, 3, 4, 5],
  overtimeIntervalMinutes: 30,
  quietHoursEnabled: true,
  quietStart: '22:30',
  quietEnd: '07:30',
  globalShortcut: 'CommandOrControl+Shift+Space',
  notificationsEnabled: true,
  autostartEnabled: false,
  persistentNotificationsEnabled: true,
  overlayRemindersEnabled: false,
  repeatUnacknowledgedEnabled: false,
  unacknowledgedRepeatMinutes: 60,
  smtpEnabled: false,
  smtpHost: '',
  smtpPort: 465,
  smtpSecurity: 'tls',
  smtpFrom: '',
  smtpTo: '',
  smtpUsername: '',
  smtpRepeatMustComplete: false,
  eventKindDefaults: [
    { eventKind: 'ORDINARY', reminderPlan: 'REPEAT' },
    { eventKind: 'ONE_TIME', reminderPlan: 'ONCE' },
    { eventKind: 'TODAY_MUST', reminderPlan: 'EMPHASIS' },
    { eventKind: 'WARNING', reminderPlan: 'REPEAT' },
    { eventKind: 'CONTINUOUS', reminderPlan: 'CUSTOM' },
    { eventKind: 'MONTHLY', reminderPlan: 'ONCE' },
    { eventKind: 'YEARLY', reminderPlan: 'ONCE' },
  ],
};

const categories: Category[] = [
  { id: 'work', name: '工作', color: '#627D98' },
  { id: 'personal', name: '个人', color: '#6E8B74' },
  { id: 'later', name: '稍后', color: '#9381A8' },
];

function readPreviewCategories(): Category[] {
  const saved = localStorage.getItem(CATEGORIES_KEY);
  return saved ? JSON.parse(saved) : categories;
}

function writePreviewCategories(values: Category[]): void {
  localStorage.setItem(CATEGORIES_KEY, JSON.stringify(values));
}

function readPreviewTags(): Tag[] {
  const saved = localStorage.getItem(TAGS_KEY);
  return saved ? JSON.parse(saved) : [];
}

function writePreviewTags(values: Tag[]): void {
  localStorage.setItem(TAGS_KEY, JSON.stringify(values));
}

function localIsoAt(time: string, dayOffset = 0): string {
  const [hours, minutes] = time.split(':').map(Number);
  const date = new Date();
  date.setDate(date.getDate() + dayOffset);
  date.setHours(hours, minutes, 0, 0);
  return date.toISOString();
}

function localTime(value: Date): string {
  return `${String(value.getHours()).padStart(2, '0')}:${String(value.getMinutes()).padStart(2, '0')}`;
}

function nextRepeatSlotIso(times: string[], now: Date): string {
  const sorted = [...times].sort();
  for (const time of sorted) {
    const [hours, minutes] = time.split(':').map(Number);
    const candidate = new Date(now);
    candidate.setHours(hours, minutes, 0, 0);
    if (candidate.getTime() > now.getTime()) return candidate.toISOString();
  }
  return localIsoAt(sorted[0], 1);
}

function mustCompleteIsoAt(settings: Settings, now: Date): string {
  const [hours, minutes] = settings.defaultDueTime.split(':').map(Number);
  const target = new Date(now);
  target.setHours(hours, minutes, 0, 0);
  return (target.getTime() > now.getTime() ? target : now).toISOString();
}

function readPreviewSettings(): Settings {
  const saved = localStorage.getItem(SETTINGS_KEY);
  return saved ? { ...defaultSettings, ...JSON.parse(saved) } : defaultSettings;
}

function readPreviewItems(): Item[] {
  const saved = localStorage.getItem(ITEMS_KEY);
  if (saved) {
    return (JSON.parse(saved) as Item[]).map((item) => ({
      ...item,
      deletedAt: item.deletedAt ?? null,
      eventKind: Object.hasOwn(item, 'eventKind')
        ? item.eventKind
        : item.completionPolicy === 'MUST_COMPLETE_TODAY'
          ? 'TODAY_MUST'
          : null,
      reminderPlan: item.reminderPlan ?? (item.completionPolicy === 'MUST_COMPLETE_TODAY' ? 'EMPHASIS' : 'ONCE'),
      important: item.important ?? false,
      timeMode: item.timeMode ?? (item.dueSource === 'EXPLICIT' ? 'SPECIFIED' : 'DEFAULT'),
      startAt: item.startAt ?? null,
      endAt: item.endAt ?? null,
      targetAt: item.targetAt ?? null,
      leadValue: item.leadValue ?? null,
      leadUnit: item.leadUnit ?? null,
      cadenceValue: item.cadenceValue ?? null,
      cadenceUnit: item.cadenceUnit ?? null,
      emphasisMaxPerDay: item.emphasisMaxPerDay ?? 8,
      repeatTimeMode: item.repeatTimeMode ?? 'SPECIFIED',
      repeatTimes: item.repeatTimes ?? (item.reminderPlan === 'REPEAT' ? [item.dueLocalTime] : []),
      tags: Array.isArray(item.tags) ? item.tags : [],
    }));
  }

  const settings = readPreviewSettings();
  const now = new Date().toISOString();
  const seed: Item[] = [
    {
      id: crypto.randomUUID(),
      title: '确认明天演示环境的访问权限',
      notes: '测试账号与数据准备完成后再通知参会人。',
      status: 'OPEN',
      categoryId: 'work',
      categoryName: '工作',
      dueAt: localIsoAt(settings.defaultDueTime),
      dueLocalDate: new Date().toISOString().slice(0, 10),
      dueLocalTime: settings.defaultDueTime,
      dueSource: 'DEFAULT_EOD',
      rolloverPolicy: 'NEXT_WORKDAY_EOD',
      rolloverCount: 0,
      completionPolicy: 'NORMAL',
      repeatIntervalMinutes: null,
      nextReminderAt: localIsoAt(settings.defaultDueTime),
      reminderPaused: false,
      bypassAppQuietHours: false,
      createdAt: now,
      updatedAt: now,
      completedAt: null,
      deletedAt: null,
      eventKind: 'ORDINARY',
      reminderPlan: 'REPEAT',
      important: false,
      timeMode: 'DEFAULT',
      startAt: null,
      endAt: null,
      targetAt: null,
      leadValue: null,
      leadUnit: null,
      cadenceValue: null,
      cadenceUnit: null,
      emphasisMaxPerDay: 8,
      repeatTimeMode: 'DEFAULT',
      repeatTimes: settings.repeatDefaultTimes,
      tags: [],
    },
    {
      id: crypto.randomUUID(),
      title: '下班前提交发布审批',
      notes: '',
      status: 'OPEN',
      categoryId: 'work',
      categoryName: '工作',
      dueAt: localIsoAt('19:30'),
      dueLocalDate: new Date().toISOString().slice(0, 10),
      dueLocalTime: '19:30',
      dueSource: 'EXPLICIT',
      rolloverPolicy: 'NONE',
      rolloverCount: 0,
      completionPolicy: 'MUST_COMPLETE_TODAY',
      repeatIntervalMinutes: 30,
      nextReminderAt: localIsoAt('19:30'),
      reminderPaused: false,
      bypassAppQuietHours: false,
      createdAt: now,
      updatedAt: now,
      completedAt: null,
      deletedAt: null,
      eventKind: 'TODAY_MUST',
      reminderPlan: 'EMPHASIS',
      important: true,
      timeMode: 'SPECIFIED',
      startAt: null,
      endAt: null,
      targetAt: null,
      leadValue: null,
      leadUnit: null,
      cadenceValue: null,
      cadenceUnit: null,
      emphasisMaxPerDay: 8,
      repeatTimeMode: 'SPECIFIED',
      repeatTimes: [],
      tags: [],
    },
  ];
  localStorage.setItem(ITEMS_KEY, JSON.stringify(seed));
  return seed;
}

function dateKey(value: Date): string {
  return `${value.getFullYear()}-${String(value.getMonth() + 1).padStart(2, '0')}-${String(value.getDate()).padStart(2, '0')}`;
}

function addDays(value: Date, days: number): Date {
  const result = new Date(value);
  result.setDate(result.getDate() + days);
  return result;
}

function previewRange(query: TimelineQuery): [Date, Date] {
  const anchor = new Date(`${query.anchorDate}T12:00:00`);
  if (query.scale === 'DAY') return [anchor, anchor];
  if (query.scale === 'WEEK') {
    const mondayOffset = (anchor.getDay() + 6) % 7;
    return [addDays(anchor, -mondayOffset), addDays(anchor, 6 - mondayOffset)];
  }
  if (query.scale === 'MONTH') {
    return [
      new Date(anchor.getFullYear(), anchor.getMonth(), 1, 12),
      new Date(anchor.getFullYear(), anchor.getMonth() + 1, 0, 12),
    ];
  }
  return [new Date(anchor.getFullYear(), 0, 1, 12), new Date(anchor.getFullYear(), 11, 31, 12)];
}

// Browser-only preview. Desktop always uses Rust's read-only timeline projection.
function previewTimeline(query: TimelineQuery): TimelineData {
  const [rangeStart, rangeEnd] = previewRange(query);
  rangeStart.setHours(0, 0, 0, 0);
  rangeEnd.setHours(0, 0, 0, 0);
  const endExclusive = addDays(rangeEnd, 1);
  const now = new Date();
  const total = endExclusive.getTime() - rangeStart.getTime();
  const point = (date: Date) => (date.getTime() - rangeStart.getTime()) / total * 100;
  const label = (date: Date) => `${dateKey(date).replaceAll('-', '/')} ${String(date.getHours()).padStart(2, '0')}:${String(date.getMinutes()).padStart(2, '0')}`;
  const span = (start: Date, end: Date) => ({
    left: Math.max(0, point(start)), width: Math.max(0, Math.min(100, point(end)) - Math.max(0, point(start))),
    clippedStart: start < rangeStart, clippedEnd: end > endExclusive, startLabel: label(start), endLabel: label(end),
  });
  const ticks: TimelineData['ticks'] = [];
  for (let date = new Date(rangeStart); date < endExclusive;) {
    ticks.push({ label: query.scale === 'DAY' ? `${String(date.getHours()).padStart(2, '0')}:00` : query.scale === 'YEAR' ? `${date.getMonth() + 1}月` : String(date.getDate()).padStart(2, '0'), position: point(date), weekend: [0, 6].includes(date.getDay()) });
    if (query.scale === 'DAY') date = new Date(date.getTime() + 3_600_000);
    else if (query.scale === 'YEAR') date.setMonth(date.getMonth() + 1);
    else date.setDate(date.getDate() + 1);
  }
  const entries: TimelineEntry[] = [];
  const unscheduled: TimelineData['unscheduled'] = [];
  for (const item of readPreviewItems()) {
    if (item.status === 'DELETED' || (!query.includeDone && item.status === 'DONE')) continue;
    if (query.eventKinds.length && (!item.eventKind || !query.eventKinds.includes(item.eventKind))) continue;
    if (query.categoryId && item.categoryId !== query.categoryId) continue;
    if (query.tagIds.length && !query.tagIds.every((id) => item.tags.some((tag) => tag.id === id))) continue;
    const created = new Date(item.createdAt);
    const endValue = item.eventKind === 'CONTINUOUS' ? item.endAt : item.eventKind === 'WARNING' ? item.targetAt : item.dueAt;
    const plannedEnd = endValue ? new Date(endValue) : item.completedAt ? new Date(item.completedAt) : now;
    const completed = item.completedAt ? new Date(item.completedAt) : null;
    if (!Number.isFinite(created.getTime()) || !Number.isFinite(plannedEnd.getTime()) || plannedEnd < created || (completed && (!Number.isFinite(completed.getTime()) || completed < created))) {
      unscheduled.push({ itemId: item.id, title: item.title, status: item.status, eventKind: item.eventKind, reason: '时间信息无法识别，请打开核对' });
      continue;
    }
    const periodic = item.eventKind === 'MONTHLY' || item.eventKind === 'YEARLY';
    function append(start: Date, finish: Date, deadline: Date | null, occurrenceLabel: string | null, futurePreview = false, allowOverdue = !periodic) {
      if (finish < rangeStart || start >= endExclusive) return;
      let highlight: TimelineEntry['highlight'] = null;
      let businessStart: Date | null = null;
      if (item.eventKind === 'CONTINUOUS' && item.startAt) {
        businessStart = new Date(Math.max(created.getTime(), new Date(item.startAt).getTime()));
        highlight = span(businessStart, finish);
      } else if (item.eventKind === 'WARNING' && item.leadValue && item.leadUnit) {
        const warningStart = new Date(finish);
        if (item.leadUnit === 'MONTH') {
          const day = warningStart.getDate();
          warningStart.setDate(1); warningStart.setMonth(warningStart.getMonth() - item.leadValue);
          warningStart.setDate(Math.min(day, new Date(warningStart.getFullYear(), warningStart.getMonth() + 1, 0).getDate()));
        } else warningStart.setDate(warningStart.getDate() - item.leadValue * (item.leadUnit === 'WEEK' ? 7 : 1));
        businessStart = new Date(Math.max(created.getTime(), warningStart.getTime()));
        highlight = span(businessStart, finish);
      }
      const stateSegments: TimelineEntry['stateSegments'] = [];
      const addState = (kind: TimelineEntry['stateSegments'][number]['kind'], text: string, from: Date, to: Date) => {
        if (to <= from) return;
        const position = span(from, to);
        stateSegments.push({ kind, label: `${text}：${position.startLabel} 至 ${position.endLabel}`, position });
      };
      if (futurePreview) {
        addState('FUTURE', '未来计划', start, finish);
      } else if (item.status === 'DONE' && !completed) {
        addState('COMPLETED', '已完成（完成时间未记录）', start, finish);
      } else {
        const activeStart = new Date(Math.min(finish.getTime(), Math.max(start.getTime(), businessStart?.getTime() ?? start.getTime())));
        const completion = item.status === 'DONE' ? completed : null;
        const recordedEnd = new Date(Math.min(activeStart.getTime(), completion?.getTime() ?? activeStart.getTime()));
        addState('RECORDED', '已记录', start, recordedEnd);
        const activeCeiling = new Date(Math.min(finish.getTime(), deadline?.getTime() ?? finish.getTime()));
        const activeEnd = new Date(Math.min(activeCeiling.getTime(), completion?.getTime() ?? activeCeiling.getTime()));
        addState('ACTIVE', '进行中', new Date(Math.min(activeStart.getTime(), activeEnd.getTime())), activeEnd);
        if (allowOverdue && deadline) {
          const overdueEnd = item.status === 'DONE' ? new Date(Math.min(finish.getTime(), completion?.getTime() ?? deadline.getTime())) : finish;
          addState('OVERDUE', '已逾期', new Date(Math.max(start.getTime(), deadline.getTime())), overdueEnd);
        }
        if (completion && completion < finish) addState('COMPLETED', '已完成', new Date(Math.max(start.getTime(), completion.getTime())), finish);
      }
      const marker = (value: Date | null, text: string) => value && point(value) >= 0 && point(value) <= 100
        ? { position: point(value), label: `${text}：${label(value)}` }
        : null;
      entries.push({
        id: `${item.id}:${start.toISOString()}`, itemId: item.id, title: item.title, status: item.status,
        eventKind: item.eventKind, categoryName: item.categoryName, tagNames: item.tags.map((tag) => tag.name),
        shape: periodic ? 'OCCURRENCE' : item.eventKind === 'WARNING' ? 'WARNING' : 'RANGE',
        startDate: dateKey(start), endDate: dateKey(finish), targetDate: deadline ? dateKey(deadline) : null,
        important: item.important, occurrenceLabel, position: span(start, finish), highlight,
        stateSegments, deadlineMarker: marker(deadline, '截止'), completionMarker: marker(completed, '完成'),
        overdue: stateSegments.some((segment) => segment.kind === 'OVERDUE'), openEnded: !deadline && item.status === 'OPEN', historyIncomplete: periodic,
      });
    }
    const displayEnd = periodic
      ? plannedEnd
      : item.status === 'DONE'
        ? new Date(Math.max(plannedEnd.getTime(), completed?.getTime() ?? plannedEnd.getTime()))
        : endValue && plannedEnd < now ? now : plannedEnd;
    append(created, displayEnd, endValue ? plannedEnd : null, periodic ? '当前已知周期' : null, false, !periodic);
    if (periodic && endValue && item.status === 'OPEN') {
      const step = item.eventKind === 'YEARLY' ? 12 : 1;
      let previous = plannedEnd;
      // Preview has no persisted anchor; never presents invented past occurrences.
      const months = (rangeStart.getFullYear() - plannedEnd.getFullYear()) * 12 + rangeStart.getMonth() - plannedEnd.getMonth();
      const first = Math.max(1, Math.floor(months / step));
      const occurrence = (index: number) => {
        const next = new Date(plannedEnd); next.setDate(1); next.setMonth(plannedEnd.getMonth() + index * step);
        next.setDate(Math.min(plannedEnd.getDate(), new Date(next.getFullYear(), next.getMonth() + 1, 0).getDate()));
        return next;
      };
      if (first > 1) previous = occurrence(first - 1);
      for (let i = first; i < first + 15 && previous < endExclusive; i++) {
        const next = occurrence(i); append(previous, next, next, `${next.getMonth() + 1}月计划`, previous >= now, false); previous = next;
      }
    }
  }
  return {
    scale: query.scale, rangeStart: dateKey(rangeStart), rangeEnd: dateKey(rangeEnd), today: dateKey(now),
    ticks, todayPosition: now >= rangeStart && now < endExclusive ? point(now) : null,
    entries: entries.slice(0, 2000), unscheduled: unscheduled.slice(0, 200),
    totalVisibleCount: entries.length, totalUnscheduledCount: unscheduled.length,
    truncated: entries.length > 2000 || unscheduled.length > 200, skippedInvalidCount: unscheduled.length,
  };
}

function writePreviewItems(items: Item[]): void {
  localStorage.setItem(ITEMS_KEY, JSON.stringify(items));
}

function previewFilter(items: Item[], filter: ItemFilter): Item[] {
  const now = new Date();
  return items.filter((item) => {
    if (filter === 'all') return item.status !== 'DELETED';
    if (filter === 'done') return item.status === 'DONE';
    if (filter === 'deleted') return item.status === 'DELETED';
    if (item.status !== 'OPEN') return false;
    if (filter === 'overdue') return new Date(item.dueAt) < now;
    if (filter === 'today') {
      const due = new Date(item.dueAt);
      return due.toDateString() === now.toDateString();
    }
    return true;
  });
}

function managedStateNotReady(cause: unknown): boolean {
  const message = typeof cause === 'string'
    ? cause
    : cause instanceof Error
      ? cause.message
      : '';
  return message.includes('state not managed for field') && message.includes('call `.manage()`');
}

function wait(milliseconds: number): Promise<void> {
  return new Promise((resolve) => window.setTimeout(resolve, milliseconds));
}

async function call<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  for (let attempt = 0; ; attempt += 1) {
    try {
      return await invoke<T>(command, args);
    } catch (cause) {
      if (managedStateNotReady(cause) && attempt < MANAGED_STATE_RETRY_DELAYS_MS.length) {
        await wait(MANAGED_STATE_RETRY_DELAYS_MS[attempt]);
        continue;
      }
      if (typeof cause === 'string') throw new Error(cause);
      throw cause;
    }
  }
}

export const api = {
  async getAppInfo(): Promise<AppInfo> {
    if (isTauri()) return call<AppInfo>('get_app_info');
    return {
      name: '闪记',
      version: '0.13.1',
      copyright: '© 2026 闪记',
      portable: false,
      updateInstallMode: 'AUTOMATIC',
    };
  },

  async getUpdateState(): Promise<UpdateState> {
    if (isTauri()) return call<UpdateState>('get_update_state');
    return readPreviewUpdateState();
  },

  async setAutoUpdateEnabled(enabled: boolean): Promise<UpdateState> {
    if (isTauri()) return call<UpdateState>('set_auto_update_enabled', { enabled });
    return writePreviewUpdateState({
      ...readPreviewUpdateState(),
      autoCheckEnabled: enabled,
      permissionPrompted: true,
    });
  },

  async dismissUpdatePermission(): Promise<UpdateState> {
    if (isTauri()) return call<UpdateState>('dismiss_update_permission');
    return writePreviewUpdateState({ ...readPreviewUpdateState(), permissionPrompted: true });
  },

  async recordUpdateCheck(result: UpdateCheckResult): Promise<UpdateState> {
    if (isTauri()) return call<UpdateState>('record_update_check', { result });
    return writePreviewUpdateState({
      ...readPreviewUpdateState(),
      lastCheckedAt: new Date().toISOString(),
      lastCheckResult: result,
    });
  },

  async snoozeUpdate(version: string): Promise<UpdateState> {
    if (isTauri()) return call<UpdateState>('snooze_update', { version });
    return writePreviewUpdateState({
      ...readPreviewUpdateState(),
      snoozedVersion: version,
      snoozedUntil: new Date(Date.now() + 24 * 60 * 60 * 1000).toISOString(),
    });
  },

  async markUpdateNotified(version: string): Promise<UpdateState> {
    if (isTauri()) return call<UpdateState>('mark_update_notified', { version });
    return writePreviewUpdateState({ ...readPreviewUpdateState(), lastNotifiedVersion: version });
  },

  async markReleaseNotesSeen(version: string): Promise<UpdateState> {
    if (isTauri()) return call<UpdateState>('mark_release_notes_seen', { version });
    return writePreviewUpdateState({
      ...readPreviewUpdateState(),
      releaseNotesSeenVersion: version,
      showCurrentReleaseNotes: false,
    });
  },

  async notifyUpdateAvailable(version: string): Promise<UpdateState> {
    if (isTauri()) return call<UpdateState>('notify_update_available', { version });
    return writePreviewUpdateState({ ...readPreviewUpdateState(), lastNotifiedVersion: version });
  },

  async openReleasePage(version: string | null = null): Promise<void> {
    if (isTauri()) {
      await call<void>('open_release_page', { version });
      return;
    }
    const suffix = version ? `/tag/v${encodeURIComponent(version)}` : '';
    window.open(`https://github.com/your-xunzong/shanji/releases${suffix}`, '_blank', 'noopener,noreferrer');
  },

  async setTrayUpdate(version: string | null): Promise<void> {
    if (isTauri()) await call<void>('set_tray_update', { version });
  },

  async listItems(filter: ItemFilter = 'open'): Promise<Item[]> {
    if (isTauri()) return call<Item[]>('list_items', { filter });
    return previewFilter(readPreviewItems(), filter).sort(
      (left, right) => new Date(left.dueAt).getTime() - new Date(right.dueAt).getTime(),
    );
  },

  async createItem(input: CreateItemInput): Promise<Item> {
    if (isTauri()) return call<Item>('create_item', { input });

    const settings = readPreviewSettings();
    const now = new Date();
    const eventKind = input.event?.kind ?? (input.mustCompleteToday ? 'TODAY_MUST' : null);
    const reminderPlan = input.event?.reminderPlan
      ?? settings.eventKindDefaults.find((entry) => entry.eventKind === eventKind)?.reminderPlan
      ?? 'REPEAT';
    const configuredTime = eventKind === 'WARNING'
      ? input.event?.targetAt
      : eventKind === 'CONTINUOUS'
        ? input.event?.startAt
        : input.dueAt;
    const dueAt = configuredTime ?? (reminderPlan === 'REPEAT'
      ? nextRepeatSlotIso(settings.repeatDefaultTimes, now)
      : eventKind === 'TODAY_MUST'
        ? mustCompleteIsoAt(settings, now)
        : localIsoAt(settings.defaultDueTime, now.getHours() >= Number(settings.defaultDueTime.slice(0, 2)) ? 1 : 0));
    const due = new Date(dueAt);
    const item: Item = {
      id: crypto.randomUUID(),
      title: input.title.trim(),
      notes: input.notes ?? '',
      status: 'OPEN',
      categoryId: input.categoryId ?? null,
      categoryName: readPreviewCategories().find((category) => category.id === input.categoryId)?.name ?? null,
      dueAt,
      dueLocalDate: localDateKey(due),
      dueLocalTime: localTime(due),
      dueSource: configuredTime ? 'EXPLICIT' : 'DEFAULT_EOD',
      rolloverPolicy: eventKind || reminderPlan !== 'ONCE' ? 'NONE' : 'NEXT_WORKDAY_EOD',
      rolloverCount: 0,
      completionPolicy: eventKind === 'TODAY_MUST' ? 'MUST_COMPLETE_TODAY' : 'NORMAL',
      repeatIntervalMinutes: reminderPlan === 'EMPHASIS' || reminderPlan === 'FORCE'
        ? input.repeatIntervalMinutes ?? settings.overtimeIntervalMinutes
        : null,
      nextReminderAt: dueAt,
      reminderPaused: false,
      bypassAppQuietHours: false,
      createdAt: now.toISOString(),
      updatedAt: now.toISOString(),
      completedAt: null,
      deletedAt: null,
      eventKind,
      reminderPlan,
      important: input.event?.important ?? false,
      timeMode: configuredTime ? 'SPECIFIED' : 'DEFAULT',
      startAt: input.event?.startAt ?? null,
      endAt: input.event?.endAt ?? null,
      targetAt: input.event?.targetAt ?? null,
      leadValue: input.event?.leadValue ?? null,
      leadUnit: input.event?.leadUnit ?? null,
      cadenceValue: input.event?.cadenceValue ?? null,
      cadenceUnit: input.event?.cadenceUnit ?? null,
      emphasisMaxPerDay: input.event?.emphasisMaxPerDay ?? 8,
      repeatTimeMode: reminderPlan === 'REPEAT'
        ? input.event?.repeatTimeMode ?? (configuredTime ? 'SPECIFIED' : 'DEFAULT')
        : 'SPECIFIED',
      repeatTimes: reminderPlan === 'REPEAT'
        ? input.event?.repeatTimeMode === 'DEFAULT'
          ? settings.repeatDefaultTimes
          : input.event?.repeatTimes.length
            ? input.event.repeatTimes
            : configuredTime
              ? [localTime(due)]
              : settings.repeatDefaultTimes
        : [],
      tags: readPreviewTags().filter((tag) => input.tagIds?.includes(tag.id)),
    };
    writePreviewItems([item, ...readPreviewItems()]);
    localStorage.removeItem(DRAFT_KEY);
    return item;
  },

  async setItemCompleted(id: string, completed: boolean): Promise<Item> {
    if (isTauri()) return call<Item>('set_item_completed', { id, completed });
    const items = readPreviewItems();
    const target = items.find((item) => item.id === id);
    if (!target) throw new Error('事项不存在');
    target.status = completed ? 'DONE' : 'OPEN';
    target.completedAt = completed ? new Date().toISOString() : null;
    target.nextReminderAt = completed ? null : target.dueAt;
    target.updatedAt = new Date().toISOString();
    writePreviewItems(items);
    return target;
  },

  async classifyItemAsOrdinary(id: string): Promise<Item> {
    if (isTauri()) return call<Item>('classify_item_as_ordinary', { id });
    const items = readPreviewItems();
    const target = items.find((item) => item.id === id);
    if (!target) throw new Error('事项不存在');
    target.eventKind = 'ORDINARY';
    target.reminderPlan = readPreviewSettings().eventKindDefaults
      .find((entry) => entry.eventKind === 'ORDINARY')?.reminderPlan ?? 'REPEAT';
    target.updatedAt = new Date().toISOString();
    writePreviewItems(items);
    return target;
  },

  async completeSeriesOccurrence(id: string): Promise<Item> {
    if (isTauri()) return call<Item>('complete_series_occurrence', { id });
    const target = readPreviewItems().find((item) => item.id === id);
    if (!target) throw new Error('事项不存在');
    return target;
  },

  async setReminderPaused(id: string, paused: boolean): Promise<Item> {
    if (isTauri()) return call<Item>('set_reminder_paused', { id, paused });
    const items = readPreviewItems();
    const target = items.find((item) => item.id === id);
    if (!target) throw new Error('事项不存在');
    target.reminderPaused = paused;
    target.nextReminderAt = paused ? null : new Date().toISOString();
    target.updatedAt = new Date().toISOString();
    writePreviewItems(items);
    return target;
  },

  async rescheduleItem(id: string, dueAt: string): Promise<Item> {
    if (isTauri()) return call<Item>('reschedule_item', { id, dueAt });
    const items = readPreviewItems();
    const target = items.find((item) => item.id === id);
    if (!target) throw new Error('事项不存在');
    if (target.status !== 'OPEN') throw new Error('只有未完成事项可以调整时间');
    const due = new Date(dueAt);
    if (Number.isNaN(due.getTime())) throw new Error('指定时间格式无效');
    target.dueAt = due.toISOString();
    target.dueLocalDate = localDateKey(due);
    target.dueLocalTime = localTime(due);
    target.dueSource = 'EXPLICIT';
    target.rolloverPolicy = 'NONE';
    target.nextReminderAt = due.toISOString();
    target.reminderPaused = false;
    target.updatedAt = new Date().toISOString();
    writePreviewItems(items);
    return target;
  },

  async getSettings(): Promise<Settings> {
    if (isTauri()) return call<Settings>('get_settings');
    return readPreviewSettings();
  },

  async updateSettings(input: UpdateSettingsInput): Promise<Settings> {
    if (isTauri()) return call<Settings>('update_settings', { input });
    const previous = readPreviewSettings();
    const { updateExistingDefaultItems: _, smtpPassword: __, ...settings } = input;
    const repeatTimes = [...settings.repeatDefaultTimes].sort();
    if (repeatTimes.length !== 2 || repeatTimes[0] === repeatTimes[1]
      || repeatTimes.some((time) => !/^(?:[01]\d|2[0-3]):[0-5]\d$/.test(time))) {
      throw new Error('请设置两个不同的有效重复提醒时间。');
    }
    settings.repeatDefaultTimes = repeatTimes;
    localStorage.setItem(SETTINGS_KEY, JSON.stringify(settings));
    if (previous.smtpHost !== settings.smtpHost
      || previous.smtpPort !== settings.smtpPort
      || previous.smtpSecurity !== settings.smtpSecurity
      || previous.smtpFrom !== settings.smtpFrom
      || previous.smtpUsername !== settings.smtpUsername
      || input.smtpPassword) {
      localStorage.removeItem(SMTP_VERIFIED_KEY);
    }
    return settings;
  },

  async listCategories(): Promise<Category[]> {
    if (isTauri()) return call<Category[]>('list_categories');
    return readPreviewCategories();
  },

  async createCategory(input: TaxonomyInput): Promise<Category> {
    if (isTauri()) return call<Category>('create_category', { input });
    const category = { id: crypto.randomUUID(), ...input };
    writePreviewCategories([...readPreviewCategories(), category]);
    return category;
  },

  async updateCategory(id: string, input: TaxonomyInput): Promise<Category> {
    if (isTauri()) return call<Category>('update_category', { id, input });
    const values = readPreviewCategories();
    const category = values.find((value) => value.id === id);
    if (!category) throw new Error('找不到要修改的类型');
    Object.assign(category, input);
    writePreviewCategories(values);
    return category;
  },

  async deleteCategory(id: string, reassignTo: string | null): Promise<void> {
    if (isTauri()) return call<void>('delete_category', { id, reassignTo });
    const categories = readPreviewCategories();
    const target = categories.find((value) => value.id === reassignTo);
    const items = readPreviewItems();
    for (const item of items) {
      if (item.categoryId === id) {
        item.categoryId = reassignTo;
        item.categoryName = target?.name ?? null;
      }
    }
    writePreviewItems(items);
    writePreviewCategories(categories.filter((value) => value.id !== id));
  },

  async moveCategory(id: string, direction: 'up' | 'down'): Promise<void> {
    if (isTauri()) return call<void>('move_category', { id, direction });
    const values = readPreviewCategories();
    const index = values.findIndex((value) => value.id === id);
    const target = direction === 'up' ? index - 1 : index + 1;
    if (index < 0 || target < 0 || target >= values.length) return;
    [values[index], values[target]] = [values[target], values[index]];
    writePreviewCategories(values);
  },

  async listTags(): Promise<Tag[]> {
    if (isTauri()) return call<Tag[]>('list_tags');
    return readPreviewTags();
  },

  async createTag(input: TaxonomyInput): Promise<Tag> {
    if (isTauri()) return call<Tag>('create_tag', { input });
    const tag = { id: crypto.randomUUID(), ...input };
    writePreviewTags([...readPreviewTags(), tag]);
    return tag;
  },

  async updateTag(id: string, input: TaxonomyInput): Promise<Tag> {
    if (isTauri()) return call<Tag>('update_tag', { id, input });
    const values = readPreviewTags();
    const tag = values.find((value) => value.id === id);
    if (!tag) throw new Error('找不到要修改的标签');
    Object.assign(tag, input);
    writePreviewTags(values);
    return tag;
  },

  async deleteTag(id: string): Promise<void> {
    if (isTauri()) return call<void>('delete_tag', { id });
    writePreviewTags(readPreviewTags().filter((tag) => tag.id !== id));
    writePreviewItems(readPreviewItems().map((item) => ({
      ...item,
      tags: item.tags.filter((tag) => tag.id !== id),
    })));
  },

  async moveTag(id: string, direction: 'up' | 'down'): Promise<void> {
    if (isTauri()) return call<void>('move_tag', { id, direction });
    const values = readPreviewTags();
    const index = values.findIndex((value) => value.id === id);
    const target = direction === 'up' ? index - 1 : index + 1;
    if (index < 0 || target < 0 || target >= values.length) return;
    [values[index], values[target]] = [values[target], values[index]];
    writePreviewTags(values);
  },

  async updateItem(id: string, input: UpdateItemInput): Promise<Item> {
    if (isTauri()) return call<Item>('update_item', { id, input });
    const items = readPreviewItems();
    const item = items.find((value) => value.id === id);
    if (!item) throw new Error('找不到事项');
    const due = new Date(input.dueAt);
    item.title = input.title.trim();
    item.notes = input.notes;
    item.categoryId = input.categoryId;
    item.categoryName = readPreviewCategories().find((value) => value.id === input.categoryId)?.name ?? null;
    item.tags = readPreviewTags().filter((tag) => input.tagIds.includes(tag.id));
    item.dueAt = due.toISOString();
    item.dueLocalDate = localDateKey(due);
    item.dueLocalTime = localTime(due);
    item.dueSource = 'EXPLICIT';
    item.rolloverPolicy = 'NONE';
    item.eventKind = input.event?.kind ?? (input.mustCompleteToday ? 'TODAY_MUST' : null);
    item.reminderPlan = input.event?.reminderPlan
      ?? readPreviewSettings().eventKindDefaults.find((entry) => entry.eventKind === item.eventKind)?.reminderPlan
      ?? 'ONCE';
    item.important = input.event?.important ?? false;
    item.timeMode = 'SPECIFIED';
    item.startAt = input.event?.startAt ?? null;
    item.endAt = input.event?.endAt ?? null;
    item.targetAt = input.event?.targetAt ?? null;
    item.leadValue = input.event?.leadValue ?? null;
    item.leadUnit = input.event?.leadUnit ?? null;
    item.cadenceValue = input.event?.cadenceValue ?? null;
    item.cadenceUnit = input.event?.cadenceUnit ?? null;
    item.emphasisMaxPerDay = input.event?.emphasisMaxPerDay ?? 8;
    item.repeatTimeMode = item.reminderPlan === 'REPEAT'
      ? input.event?.repeatTimeMode ?? 'SPECIFIED'
      : item.repeatTimeMode;
    item.repeatTimes = item.reminderPlan === 'REPEAT'
      ? item.repeatTimeMode === 'DEFAULT'
        ? readPreviewSettings().repeatDefaultTimes
        : input.event?.repeatTimes.length
          ? input.event.repeatTimes.slice(0, 1)
          : [localTime(due)]
      : item.repeatTimes;
    item.completionPolicy = item.eventKind === 'TODAY_MUST' ? 'MUST_COMPLETE_TODAY' : 'NORMAL';
    item.repeatIntervalMinutes = item.reminderPlan === 'EMPHASIS' || item.reminderPlan === 'FORCE'
      ? input.repeatIntervalMinutes ?? 30
      : null;
    item.nextReminderAt = item.status === 'OPEN' ? due.toISOString() : null;
    item.updatedAt = new Date().toISOString();
    writePreviewItems(items);
    return item;
  },

  async setItemDeleted(id: string, deleted: boolean): Promise<Item> {
    if (isTauri()) return call<Item>('set_item_deleted', { id, deleted });
    const items = readPreviewItems();
    const item = items.find((value) => value.id === id);
    if (!item) throw new Error('找不到事项');
    item.status = deleted ? 'DELETED' : 'OPEN';
    item.deletedAt = deleted ? new Date().toISOString() : null;
    item.nextReminderAt = deleted ? null : item.dueAt;
    writePreviewItems(items);
    return item;
  },

  async permanentlyDeleteItem(id: string): Promise<void> {
    if (isTauri()) return call<void>('permanently_delete_item', { id });
    writePreviewItems(readPreviewItems().filter((item) => item.id !== id));
  },

  async listPendingReminders(): Promise<Item[]> {
    if (isTauri()) return call<Item[]>('list_pending_reminders');
    const acknowledged = new Set<string>(JSON.parse(localStorage.getItem(REMINDER_ACKS_KEY) ?? '[]'));
    const now = Date.now();
    return readPreviewItems().filter((item) => item.status === 'OPEN'
      && !item.reminderPaused
      && new Date(item.dueAt).getTime() <= now
      && !acknowledged.has(item.id));
  },

  async acknowledgeReminder(id: string): Promise<Item> {
    if (isTauri()) return call<Item>('acknowledge_reminder', { id });
    const values = new Set<string>(JSON.parse(localStorage.getItem(REMINDER_ACKS_KEY) ?? '[]'));
    values.add(id);
    localStorage.setItem(REMINDER_ACKS_KEY, JSON.stringify([...values]));
    const item = readPreviewItems().find((value) => value.id === id);
    if (!item) throw new Error('找不到事项');
    return item;
  },

  async snoozeItem(id: string, minutes: number): Promise<Item> {
    if (isTauri()) return call<Item>('snooze_item', { id, minutes });
    const items = readPreviewItems();
    const item = items.find((value) => value.id === id);
    if (!item) throw new Error('找不到事项');
    item.nextReminderAt = new Date(Date.now() + minutes * 60_000).toISOString();
    item.updatedAt = new Date().toISOString();
    writePreviewItems(items);
    await api.acknowledgeReminder(id);
    return item;
  },

  async listEmailRouteTagIds(): Promise<string[]> {
    if (isTauri()) return call<string[]>('list_email_route_tag_ids');
    return JSON.parse(localStorage.getItem(EMAIL_ROUTES_KEY) ?? '[]');
  },

  async setTagEmailRoute(tagId: string, enabled: boolean): Promise<void> {
    if (isTauri()) return call<void>('set_tag_email_route', { tagId, enabled });
    const values = new Set<string>(JSON.parse(localStorage.getItem(EMAIL_ROUTES_KEY) ?? '[]'));
    if (enabled) values.add(tagId);
    else values.delete(tagId);
    localStorage.setItem(EMAIL_ROUTES_KEY, JSON.stringify([...values]));
  },

  async listEmailDeliveryRules(): Promise<EmailDeliveryRule[]> {
    if (isTauri()) return call<EmailDeliveryRule[]>('list_email_delivery_rules');
    return JSON.parse(localStorage.getItem(EMAIL_RULES_KEY) ?? '[]');
  },

  async saveEmailDeliveryRule(input: EmailDeliveryRuleInput): Promise<EmailDeliveryRule> {
    if (isTauri()) return call<EmailDeliveryRule>('save_email_delivery_rule', { input });
    if (input.enabled && !localStorage.getItem(SMTP_VERIFIED_KEY)) {
      throw new Error('请先在后台设置中发送测试邮件，确认连接可用后再启用规则。');
    }
    const rules = JSON.parse(localStorage.getItem(EMAIL_RULES_KEY) ?? '[]') as EmailDeliveryRule[];
    const now = new Date().toISOString();
    const existing = input.id ? rules.find((rule) => rule.id === input.id) : undefined;
    const rule: EmailDeliveryRule = {
      ...input,
      id: input.id ?? crypto.randomUUID(),
      createdAt: existing?.createdAt ?? now,
      updatedAt: now,
    };
    const next = existing ? rules.map((value) => value.id === rule.id ? rule : value) : [...rules, rule];
    localStorage.setItem(EMAIL_RULES_KEY, JSON.stringify(next));
    return rule;
  },

  async deleteEmailDeliveryRule(id: string): Promise<void> {
    if (isTauri()) return call<void>('delete_email_delivery_rule', { id });
    const rules = JSON.parse(localStorage.getItem(EMAIL_RULES_KEY) ?? '[]') as EmailDeliveryRule[];
    localStorage.setItem(EMAIL_RULES_KEY, JSON.stringify(rules.filter((rule) => rule.id !== id)));
  },

  async exportExcel(path: string, filter: ExportFilterInput): Promise<ExportResult> {
    if (isTauri()) return call<ExportResult>('export_excel', { path, filter });
    const items = readPreviewItems().filter((item) => {
      const created = new Date(item.createdAt).getTime();
      return (filter.status === 'all' || item.status === filter.status.toUpperCase())
        && (!filter.eventKind || item.eventKind === filter.eventKind)
        && (!filter.categoryId || item.categoryId === filter.categoryId)
        && (!filter.tagId || item.tags.some((tag) => tag.id === filter.tagId))
        && (!filter.createdFrom || created >= new Date(filter.createdFrom).getTime())
        && (!filter.createdTo || created <= new Date(filter.createdTo).getTime())
        && item.status !== 'DELETED';
    });
    return { path, itemCount: items.length };
  },

  async loadDraft(): Promise<string> {
    if (isTauri()) return call<string>('load_draft');
    return localStorage.getItem(DRAFT_KEY) ?? '';
  },

  async saveDraft(content: string): Promise<void> {
    if (isTauri()) return call<void>('save_draft', { content });
    localStorage.setItem(DRAFT_KEY, content);
  },

  async hideCapture(): Promise<void> {
    if (isTauri()) await call<void>('hide_capture');
  },

  async showCapture(): Promise<void> {
    if (isTauri()) await call<void>('show_capture');
  },

  async showMain(): Promise<void> {
    if (isTauri()) await call<void>('show_main');
  },

  async hideReminder(): Promise<void> {
    if (isTauri()) await call<void>('hide_reminder');
  },

  async getSystemWarning(): Promise<string | null> {
    if (isTauri()) return call<string | null>('get_system_warning');
    return null;
  },

  async getAutostartStatus(): Promise<AutostartStatus> {
    if (isTauri()) return call<AutostartStatus>('get_autostart_status');
    const settings = readPreviewSettings();
    return {
      enabled: settings.autostartEnabled,
      available: true,
      portable: false,
      message: settings.autostartEnabled
        ? '浏览器预览中的开机启动已启用。'
        : '浏览器预览中的开机启动未启用。',
    };
  },

  async getOnboardingStatus(): Promise<OnboardingStatus> {
    if (isTauri()) return call<OnboardingStatus>('get_onboarding_status');
    const completedVersion = Number(localStorage.getItem(ONBOARDING_KEY) ?? 0);
    return { required: completedVersion < 3, completedVersion, currentVersion: 3 };
  },

  async completeOnboarding(): Promise<OnboardingStatus> {
    if (isTauri()) return call<OnboardingStatus>('complete_onboarding');
    localStorage.setItem(ONBOARDING_KEY, '3');
    return { required: false, completedVersion: 3, currentVersion: 3 };
  },

  async getNotificationStatus(): Promise<NotificationStatus> {
    if (isTauri()) return call<NotificationStatus>('get_notification_status');
    return {
      platform: 'browser',
      portable: false,
      identityStatus: 'ready',
      canNotify: true,
      message: '浏览器预览不代表桌面系统通知状态。',
      lastResult: null,
      lastErrorCode: null,
      lastAttemptAt: null,
    };
  },

  async getSmtpStatus(): Promise<SmtpStatus> {
    if (isTauri()) return call<SmtpStatus>('get_smtp_status');
    return {
      passwordConfigured: false,
      verifiedAt: localStorage.getItem(SMTP_VERIFIED_KEY),
      lastResult: null,
      lastErrorCode: null,
      lastAttemptAt: null,
    };
  },

  async testSmtp(): Promise<string> {
    if (isTauri()) return call<string>('test_smtp');
    const settings = readPreviewSettings();
    if (!settings.smtpEnabled) throw new Error('请先启用邮件通知并保存设置。');
    localStorage.setItem(SMTP_VERIFIED_KEY, new Date().toISOString());
    return `浏览器预览不会连接邮件服务器；桌面版将发送到 ${settings.smtpTo}。`;
  },

  async registerPortableNotifications(): Promise<NotificationStatus> {
    if (isTauri()) return call<NotificationStatus>('register_portable_notifications');
    return this.getNotificationStatus();
  },

  async unregisterPortableNotifications(): Promise<NotificationStatus> {
    if (isTauri()) return call<NotificationStatus>('unregister_portable_notifications');
    return this.getNotificationStatus();
  },

  async createNotificationTestItem(): Promise<Item> {
    if (isTauri()) return call<Item>('create_notification_test_item');
    const now = new Date();
    const dueAt = new Date(now.getTime() + 10_000);
    const item: Item = {
      id: crypto.randomUUID(),
      title: '通知测试：闪记提醒功能正常',
      notes: '这是一条本地测试事项，可在确认通知后将其标记完成。',
      status: 'OPEN',
      categoryId: null,
      categoryName: null,
      dueAt: dueAt.toISOString(),
      dueLocalDate: localDateKey(dueAt),
      dueLocalTime: localTime(dueAt),
      dueSource: 'EXPLICIT',
      rolloverPolicy: 'NONE',
      rolloverCount: 0,
      completionPolicy: 'NORMAL',
      repeatIntervalMinutes: null,
      nextReminderAt: dueAt.toISOString(),
      reminderPaused: false,
      bypassAppQuietHours: false,
      createdAt: now.toISOString(),
      updatedAt: now.toISOString(),
      completedAt: null,
      deletedAt: null,
      eventKind: 'ONE_TIME',
      reminderPlan: 'ONCE',
      important: false,
      timeMode: 'SPECIFIED',
      startAt: null,
      endAt: null,
      targetAt: null,
      leadValue: null,
      leadUnit: null,
      cadenceValue: null,
      cadenceUnit: null,
      emphasisMaxPerDay: 8,
      repeatTimeMode: 'SPECIFIED',
      repeatTimes: [],
      tags: [],
    };
    writePreviewItems([item, ...readPreviewItems()]);
    return item;
  },

  async testReminderMode(mode: 'persistent' | 'overlay'): Promise<string> {
    if (isTauri()) return call<string>('test_reminder_mode', { mode });
    if (mode === 'overlay') return '置顶提醒窗测试需要在桌面版中查看。';
    return `${mode} 测试已准备。`;
  },

  async getDataStatus(): Promise<DataStatus> {
    if (isTauri()) return call<DataStatus>('get_data_status');
    const items = readPreviewItems();
    return {
      current: {
        path: '浏览器预览数据',
        itemCount: items.length,
        reminderHistoryCount: 0,
        hasSettings: true,
        hasDraft: false,
        hasCustomSettings: false,
        schemaVersion: 10,
        updatedAt: new Date().toISOString(),
      },
      latestBackup: null,
      recoveryCandidates: [],
    };
  },

  async createDataBackup(): Promise<DataFileSummary> {
    if (isTauri()) return call<DataFileSummary>('create_data_backup');
    return {
      path: '浏览器预览不创建文件备份',
      itemCount: readPreviewItems().length,
      reminderHistoryCount: 0,
      hasSettings: true,
      hasDraft: false,
      hasCustomSettings: false,
      schemaVersion: 10,
      updatedAt: new Date().toISOString(),
    };
  },

  async openDataDirectory(): Promise<void> {
    if (isTauri()) await call<void>('open_data_directory');
  },

  async getRepositoryStatus(): Promise<RepositoryStatus> {
    if (isTauri()) return call<RepositoryStatus>('get_repository_status');
    const path = localStorage.getItem(REPOSITORY_KEY);
    return {
      configured: Boolean(path),
      path,
      available: Boolean(path),
      localItemCount: readPreviewItems().length,
      packageCount: path ? 2 : 0,
      pendingSourceCount: path ? 1 : 0,
      conflictCount: 0,
      lastSyncedAt: localStorage.getItem(REPOSITORY_SYNC_KEY),
      lastPackageAt: path ? localStorage.getItem(REPOSITORY_SYNC_KEY) ?? new Date().toISOString() : null,
      snapshotReady: Boolean(path),
      lastErrorCode: null,
      message: path ? '个人仓库可用，可以预览并汇入其他设备的记录。' : '尚未选择个人仓库位置。',
    };
  },

  async configureDataRepository(path: string): Promise<RepositoryStatus> {
    if (isTauri()) return call<RepositoryStatus>('configure_data_repository', { path });
    localStorage.setItem(REPOSITORY_KEY, `${path.replace(/[\\/]$/, '')}\\shanji-repository`);
    return this.getRepositoryStatus();
  },

  async disableDataRepository(): Promise<RepositoryStatus> {
    if (isTauri()) return call<RepositoryStatus>('disable_data_repository');
    localStorage.removeItem(REPOSITORY_KEY);
    return this.getRepositoryStatus();
  },

  async previewRepositoryMerge(): Promise<RepositoryPreview> {
    if (isTauri()) return call<RepositoryPreview>('preview_repository_merge');
    if (!localStorage.getItem(REPOSITORY_KEY)) throw new Error('请先选择个人仓库位置。');
    return {
      sources: [{
        sourceInstanceId: '另一台设备',
        exportedAt: new Date(Date.now() - 3_600_000).toISOString(),
        itemCount: 6,
        newItems: 2,
        unchangedItems: 3,
        conflicts: 1,
        tombstones: 0,
      }],
      newItems: 2,
      unchangedItems: 3,
      conflicts: 1,
      tombstones: 0,
      settingsNeedReview: false,
    };
  },

  async publishDataRepositorySnapshot(): Promise<RepositoryStatus> {
    if (isTauri()) return call<RepositoryStatus>('publish_data_repository_snapshot');
    if (!localStorage.getItem(REPOSITORY_KEY)) throw new Error('请先选择个人仓库位置。');
    localStorage.setItem(REPOSITORY_SYNC_KEY, new Date().toISOString());
    return this.getRepositoryStatus();
  },

  async syncDataRepository(): Promise<RepositorySyncResult> {
    if (isTauri()) return call<RepositorySyncResult>('sync_data_repository');
    if (!localStorage.getItem(REPOSITORY_KEY)) throw new Error('请先选择个人仓库位置。');
    const syncedAt = new Date().toISOString();
    localStorage.setItem(REPOSITORY_SYNC_KEY, syncedAt);
    return {
      importedItems: 2,
      unchangedItems: 3,
      conflicts: 1,
      tombstonesApplied: 0,
      packagePath: `${localStorage.getItem(REPOSITORY_KEY)}\\preview.sjpack`,
      packageWritten: true,
      syncedAt,
    };
  },

  async openRepositoryDirectory(): Promise<void> {
    if (isTauri()) await call<void>('open_repository_directory');
  },

  async getTimeline(query: TimelineQuery): Promise<TimelineData> {
    if (isTauri()) return call<TimelineData>('get_timeline', { query });
    return previewTimeline(query);
  },

  async saveTimelineView(scale: TimelineQuery['scale'], includeDone: boolean): Promise<void> {
    if (isTauri()) await call<void>('save_timeline_view', { scale, includeDone });
  },

  async restoreDatabase(candidatePath: string): Promise<void> {
    if (isTauri()) return call<void>('restore_database', { candidatePath });
    throw new Error('浏览器预览不能恢复桌面数据。');
  },

  async getStartupStatus(): Promise<StartupStatus> {
    if (isTauri()) return call<StartupStatus>('get_startup_status');
    const recoveryPreview = new URLSearchParams(window.location.search).get('previewRecovery');
    if (recoveryPreview === 'found') {
      return {
        mode: 'recovery_required',
        targetPath: 'C:\\Users\\Lin\\AppData\\Roaming\\com.shanji.desktop\\shanji.db',
        message: '当前数据位置没有记录，但找到了以前保存的内容。恢复前不会写入空数据。',
        current: null,
        recoveryCandidates: [{
          path: 'D:\\闪记旧版\\data\\shanji.db',
          itemCount: 128,
          reminderHistoryCount: 346,
          hasSettings: true,
          hasDraft: true,
          hasCustomSettings: true,
          schemaVersion: 10,
          updatedAt: '2026-09-01T10:20:00Z',
        }],
        diagnostic: 'startup_state=legacy_data_found; target_exists=false; target_bytes=0; current_schema=unknown; candidates=1; app_schema=10',
      };
    }
    if (recoveryPreview === 'blocked') {
      return {
        mode: 'blocked',
        targetPath: 'C:\\Users\\Lin\\AppData\\Roaming\\com.shanji.desktop\\shanji.db',
        message: '原数据仍在，但可用空间不足，无法安全完成备份或升级。释放空间后可以重试。',
        current: null,
        recoveryCandidates: [],
        diagnostic: 'startup_state=disk_full; target_exists=true; target_bytes=8421376; current_schema=10; candidates=0; app_schema=10',
      };
    }
    return {
      mode: 'ready',
      targetPath: '浏览器预览数据',
      message: '',
      current: null,
      recoveryCandidates: [],
      diagnostic: 'startup_state=ready; preview=true',
    };
  },

  async restoreStartupDatabase(candidatePath: string): Promise<void> {
    if (isTauri()) return call<void>('restore_startup_database', { candidatePath });
    throw new Error('浏览器预览不能恢复桌面数据。');
  },

  async openStartupDataDirectory(path: string): Promise<void> {
    if (isTauri()) await call<void>('open_startup_data_directory', { path });
  },

  async retryStartup(): Promise<void> {
    if (isTauri()) return call<void>('retry_startup');
    window.location.reload();
  },
};
