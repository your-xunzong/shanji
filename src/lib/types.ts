export type ItemStatus = 'OPEN' | 'DONE' | 'ARCHIVED' | 'DELETED';
export type DueSource = 'EXPLICIT' | 'DEFAULT_EOD' | 'ROLLED_OVER';
export type CompletionPolicy = 'NORMAL' | 'MUST_COMPLETE_TODAY';
export type ItemFilter = 'open' | 'today' | 'overdue' | 'done' | 'all';

export interface Item {
  id: string;
  title: string;
  notes: string;
  status: ItemStatus;
  categoryId: string | null;
  categoryName: string | null;
  dueAt: string;
  dueLocalDate: string;
  dueLocalTime: string;
  dueSource: DueSource;
  rolloverPolicy: 'NONE' | 'NEXT_WORKDAY_EOD' | 'DAILY';
  rolloverCount: number;
  completionPolicy: CompletionPolicy;
  repeatIntervalMinutes: number | null;
  nextReminderAt: string | null;
  reminderPaused: boolean;
  bypassAppQuietHours: boolean;
  createdAt: string;
  updatedAt: string;
  completedAt: string | null;
}

export interface Settings {
  defaultDueTime: string;
  workdays: number[];
  overtimeIntervalMinutes: number;
  quietHoursEnabled: boolean;
  quietStart: string;
  quietEnd: string;
  globalShortcut: string;
  notificationsEnabled: boolean;
}

export interface CreateItemInput {
  title: string;
  notes?: string;
  categoryId?: string | null;
  dueAt?: string | null;
  mustCompleteToday: boolean;
  repeatIntervalMinutes?: number | null;
}

export interface UpdateSettingsInput extends Settings {
  updateExistingDefaultItems: boolean;
}

export interface Category {
  id: string;
  name: string;
  color: string;
}

