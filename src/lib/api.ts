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
const MANAGED_STATE_RETRY_DELAYS_MS = [25, 50, 100, 200, 400, 800];

const defaultSettings: Settings = {
  defaultDueTime: '18:00',
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
      tags: [],
    },
  ];
  localStorage.setItem(ITEMS_KEY, JSON.stringify(seed));
  return seed;
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
    const dueAt = input.dueAt ?? (input.mustCompleteToday
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
      dueSource: input.dueAt ? 'EXPLICIT' : 'DEFAULT_EOD',
      rolloverPolicy: input.mustCompleteToday ? 'NONE' : 'NEXT_WORKDAY_EOD',
      rolloverCount: 0,
      completionPolicy: input.mustCompleteToday ? 'MUST_COMPLETE_TODAY' : 'NORMAL',
      repeatIntervalMinutes: input.mustCompleteToday
        ? input.repeatIntervalMinutes ?? settings.overtimeIntervalMinutes
        : null,
      nextReminderAt: dueAt,
      reminderPaused: false,
      bypassAppQuietHours: false,
      createdAt: now.toISOString(),
      updatedAt: now.toISOString(),
      completedAt: null,
      deletedAt: null,
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
    const { updateExistingDefaultItems: _, smtpPassword: __, ...settings } = input;
    localStorage.setItem(SETTINGS_KEY, JSON.stringify(settings));
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
    item.completionPolicy = input.mustCompleteToday ? 'MUST_COMPLETE_TODAY' : 'NORMAL';
    item.repeatIntervalMinutes = input.mustCompleteToday ? input.repeatIntervalMinutes ?? 30 : null;
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

  async exportExcel(path: string, filter: ExportFilterInput): Promise<ExportResult> {
    if (isTauri()) return call<ExportResult>('export_excel', { path, filter });
    const items = readPreviewItems().filter((item) => {
      const created = new Date(item.createdAt).getTime();
      return (filter.status === 'all' || item.status === filter.status.toUpperCase())
        && (!filter.categoryId || (filter.categoryId === '__inbox__' ? !item.categoryId : item.categoryId === filter.categoryId))
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
    return { required: completedVersion < 2, completedVersion, currentVersion: 2 };
  },

  async completeOnboarding(): Promise<OnboardingStatus> {
    if (isTauri()) return call<OnboardingStatus>('complete_onboarding');
    localStorage.setItem(ONBOARDING_KEY, '2');
    return { required: false, completedVersion: 2, currentVersion: 2 };
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
      lastResult: null,
      lastErrorCode: null,
      lastAttemptAt: null,
    };
  },

  async testSmtp(): Promise<string> {
    if (isTauri()) return call<string>('test_smtp');
    const settings = readPreviewSettings();
    if (!settings.smtpEnabled) throw new Error('请先启用邮件通知并保存设置。');
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
      tags: [],
    };
    writePreviewItems([item, ...readPreviewItems()]);
    return item;
  },

  async testReminderMode(mode: 'standard' | 'persistent' | 'overlay' | 'repeat' | 'center'): Promise<string> {
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
        schemaVersion: 4,
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
      schemaVersion: 4,
      updatedAt: new Date().toISOString(),
    };
  },

  async openDataDirectory(): Promise<void> {
    if (isTauri()) await call<void>('open_data_directory');
  },

  async restoreDatabase(candidatePath: string): Promise<void> {
    if (isTauri()) return call<void>('restore_database', { candidatePath });
    throw new Error('浏览器预览不能恢复桌面数据。');
  },
};
