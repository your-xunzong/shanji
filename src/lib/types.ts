export type ItemStatus = 'OPEN' | 'DONE' | 'ARCHIVED' | 'DELETED';
export type DueSource = 'EXPLICIT' | 'DEFAULT_EOD' | 'ROLLED_OVER';
export type CompletionPolicy = 'NORMAL' | 'MUST_COMPLETE_TODAY';
export type ItemFilter = 'open' | 'today' | 'overdue' | 'done' | 'deleted' | 'all';

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
  deletedAt: string | null;
  tags: Tag[];
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
  autostartEnabled: boolean;
}

export interface CreateItemInput {
  title: string;
  notes?: string;
  categoryId?: string | null;
  dueAt?: string | null;
  mustCompleteToday: boolean;
  repeatIntervalMinutes?: number | null;
  tagIds?: string[];
}

export interface UpdateItemInput {
  title: string;
  notes: string;
  categoryId: string | null;
  tagIds: string[];
  dueAt: string;
  mustCompleteToday: boolean;
  repeatIntervalMinutes: number | null;
}

export interface UpdateSettingsInput extends Settings {
  updateExistingDefaultItems: boolean;
}

export interface Category {
  id: string;
  name: string;
  color: string;
}

export interface Tag {
  id: string;
  name: string;
  color: string;
}

export interface TaxonomyInput {
  name: string;
  color: string;
}

export interface NotificationStatus {
  platform: string;
  portable: boolean;
  identityStatus: 'ready' | 'registration_required' | 'unavailable';
  canNotify: boolean;
  message: string;
  lastResult: 'CLAIMED' | 'SUBMITTED' | 'FAILED' | 'DISABLED' | null;
  lastErrorCode: string | null;
  lastAttemptAt: string | null;
}

export interface AutostartStatus {
  enabled: boolean;
  available: boolean;
  portable: boolean;
  message: string;
}

export interface OnboardingStatus {
  required: boolean;
  completedVersion: number;
  currentVersion: number;
}

export interface OnboardingFinishInput {
  autostartEnabled: boolean;
  defaultDueTime: string;
  globalShortcut: string;
  enablePortableNotifications: boolean;
}

export interface DataFileSummary {
  path: string;
  itemCount: number;
  schemaVersion: number;
  updatedAt: string | null;
}

export interface DataStatus {
  current: DataFileSummary;
  latestBackup: DataFileSummary | null;
  recoveryCandidates: DataFileSummary[];
}

export interface ExportResult {
  path: string;
  itemCount: number;
}

export interface ExportFilterInput {
  status: 'all' | 'open' | 'done';
  categoryId: string | null;
  tagId: string | null;
  createdFrom: string | null;
  createdTo: string | null;
}
