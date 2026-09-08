export type ItemStatus = 'OPEN' | 'DONE' | 'ARCHIVED' | 'DELETED';
export type DueSource = 'EXPLICIT' | 'DEFAULT_EOD' | 'ROLLED_OVER';
export type CompletionPolicy = 'NORMAL' | 'MUST_COMPLETE_TODAY';
export type ItemFilter = 'open' | 'today' | 'overdue' | 'done' | 'deleted' | 'all';
export type EventKind = 'ORDINARY' | 'ONE_TIME' | 'TODAY_MUST' | 'WARNING' | 'CONTINUOUS' | 'MONTHLY' | 'YEARLY';
export type ReminderPlan = 'REPEAT' | 'EMPHASIS' | 'ONCE' | 'FORCE' | 'CUSTOM';
export type ScheduleUnit = 'DAY' | 'WEEK' | 'MONTH' | 'YEAR';
export type RepeatTimeMode = 'DEFAULT' | 'SPECIFIED';

export interface EventKindDefault {
  eventKind: EventKind;
  reminderPlan: ReminderPlan;
}

export interface EventConfigurationInput {
  kind: EventKind | null;
  reminderPlan: ReminderPlan | null;
  important: boolean;
  startAt: string | null;
  endAt: string | null;
  targetAt: string | null;
  leadValue: number | null;
  leadUnit: ScheduleUnit | null;
  cadenceValue: number | null;
  cadenceUnit: ScheduleUnit | null;
  emphasisMaxPerDay: number | null;
  repeatTimeMode: RepeatTimeMode | null;
  repeatTimes: string[];
}

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
  eventKind: EventKind | null;
  reminderPlan: ReminderPlan;
  important: boolean;
  timeMode: 'DEFAULT' | 'SPECIFIED';
  startAt: string | null;
  endAt: string | null;
  targetAt: string | null;
  leadValue: number | null;
  leadUnit: ScheduleUnit | null;
  cadenceValue: number | null;
  cadenceUnit: ScheduleUnit | null;
  emphasisMaxPerDay: number;
  repeatTimeMode: RepeatTimeMode;
  repeatTimes: string[];
  tags: Tag[];
}

export interface Settings {
  defaultDueTime: string;
  repeatDefaultTimes: string[];
  workdays: number[];
  overtimeIntervalMinutes: number;
  quietHoursEnabled: boolean;
  quietStart: string;
  quietEnd: string;
  globalShortcut: string;
  notificationsEnabled: boolean;
  autostartEnabled: boolean;
  persistentNotificationsEnabled: boolean;
  overlayRemindersEnabled: boolean;
  repeatUnacknowledgedEnabled: boolean;
  unacknowledgedRepeatMinutes: number;
  smtpEnabled: boolean;
  smtpHost: string;
  smtpPort: number;
  smtpSecurity: 'tls' | 'starttls';
  smtpFrom: string;
  smtpTo: string;
  smtpUsername: string;
  smtpRepeatMustComplete: boolean;
  eventKindDefaults: EventKindDefault[];
}

export interface CreateItemInput {
  title: string;
  notes?: string;
  categoryId?: string | null;
  dueAt?: string | null;
  mustCompleteToday: boolean;
  repeatIntervalMinutes?: number | null;
  tagIds?: string[];
  event?: EventConfigurationInput | null;
}

export interface UpdateItemInput {
  title: string;
  notes: string;
  categoryId: string | null;
  tagIds: string[];
  dueAt: string;
  mustCompleteToday: boolean;
  repeatIntervalMinutes: number | null;
  event?: EventConfigurationInput | null;
}

export interface UpdateSettingsInput extends Settings {
  updateExistingDefaultItems: boolean;
  smtpPassword?: string | null;
}

export interface DueNotification {
  eventId: string;
  itemId: string;
  title: string;
  dueLocalDate: string;
  dueLocalTime: string;
  completionPolicy: CompletionPolicy;
  eventKind: EventKind | null;
  reminderPlan: ReminderPlan;
  tagIds: string[];
}

export interface AppInfo {
  name: string;
  version: string;
  copyright: string;
  portable: boolean;
  updateInstallMode: 'AUTOMATIC' | 'DOWNLOAD_ONLY';
}

export type UpdateCheckResult = 'UP_TO_DATE' | 'UPDATE_AVAILABLE' | 'FAILED';

export interface UpdateState {
  autoCheckEnabled: boolean;
  permissionPrompted: boolean;
  lastCheckedAt: string | null;
  lastCheckResult: UpdateCheckResult | null;
  snoozedVersion: string | null;
  snoozedUntil: string | null;
  lastNotifiedVersion: string | null;
  releaseNotesSeenVersion: string | null;
  shouldAutoCheck: boolean;
  showCurrentReleaseNotes: boolean;
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

export interface SmtpStatus {
  passwordConfigured: boolean;
  verifiedAt: string | null;
  lastResult: 'CLAIMED' | 'SUBMITTED' | 'FAILED' | null;
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
  repeatDefaultTimes: string[];
  globalShortcut: string;
  enablePortableNotifications: boolean;
}

export interface DataFileSummary {
  path: string;
  itemCount: number;
  reminderHistoryCount: number;
  hasSettings: boolean;
  hasDraft: boolean;
  hasCustomSettings: boolean;
  schemaVersion: number;
  updatedAt: string | null;
}

export interface DataStatus {
  current: DataFileSummary;
  latestBackup: DataFileSummary | null;
  recoveryCandidates: DataFileSummary[];
}

export type StartupMode = 'ready' | 'recovery_required' | 'blocked';

export interface StartupStatus {
  mode: StartupMode;
  targetPath: string;
  message: string;
  current: DataFileSummary | null;
  recoveryCandidates: DataFileSummary[];
  diagnostic: string;
}

export interface ExportResult {
  path: string;
  itemCount: number;
}

export interface ExportFilterInput {
  status: 'all' | 'open' | 'done';
  eventKind: EventKind | null;
  categoryId: string | null;
  tagId: string | null;
  createdFrom: string | null;
  createdTo: string | null;
}

export type EmailRuleDimension = 'EVENT_KIND' | 'CATEGORY' | 'TAG';
export type EmailDeliveryStrategy = 'FIRST_DUE' | 'DAILY_FIRST' | 'EACH_PLAN' | 'DAILY_DIGEST';

export interface EmailDeliveryRule {
  id: string;
  matchDimension: EmailRuleDimension;
  matchValue: string;
  strategy: EmailDeliveryStrategy;
  recipient: string;
  digestLocalTime: string;
  enabled: boolean;
  createdAt: string;
  updatedAt: string;
}

export interface EmailDeliveryRuleInput {
  id: string | null;
  matchDimension: EmailRuleDimension;
  matchValue: string;
  strategy: EmailDeliveryStrategy;
  recipient: string;
  digestLocalTime: string;
  enabled: boolean;
}

export interface RepositorySourcePreview {
  sourceInstanceId: string;
  exportedAt: string;
  itemCount: number;
  newItems: number;
  unchangedItems: number;
  conflicts: number;
  tombstones: number;
}

export interface RepositoryPreview {
  sources: RepositorySourcePreview[];
  newItems: number;
  unchangedItems: number;
  conflicts: number;
  tombstones: number;
  settingsNeedReview: boolean;
}

export interface RepositoryStatus {
  configured: boolean;
  path: string | null;
  available: boolean;
  localItemCount: number;
  packageCount: number;
  pendingSourceCount: number;
  conflictCount: number;
  lastSyncedAt: string | null;
  lastPackageAt: string | null;
  snapshotReady: boolean;
  lastErrorCode: string | null;
  message: string;
}

export interface RepositorySyncResult {
  importedItems: number;
  unchangedItems: number;
  conflicts: number;
  tombstonesApplied: number;
  packagePath: string | null;
  packageWritten: boolean;
  syncedAt: string;
}

export type TimelineScale = 'DAY' | 'WEEK' | 'MONTH' | 'YEAR';

export interface TimelineQuery {
  scale: TimelineScale;
  anchorDate: string;
  includeDone: boolean;
  eventKinds: EventKind[];
  categoryId: string | null;
  tagIds: string[];
}

export interface TimelinePosition {
  left: number;
  width: number;
  clippedStart: boolean;
  clippedEnd: boolean;
  startLabel: string;
  endLabel: string;
}

export interface TimelineStateSegment {
  kind: 'RECORDED' | 'ACTIVE' | 'OVERDUE' | 'COMPLETED' | 'FUTURE';
  label: string;
  position: TimelinePosition;
}

export interface TimelineMarker {
  position: number;
  label: string;
}

export interface TimelineEntry {
  id: string;
  itemId: string;
  title: string;
  status: ItemStatus;
  eventKind: EventKind | null;
  categoryName: string | null;
  tagNames: string[];
  shape: 'RANGE' | 'WARNING' | 'OCCURRENCE';
  startDate: string;
  endDate: string;
  targetDate: string | null;
  important: boolean;
  occurrenceLabel: string | null;
  position: TimelinePosition;
  highlight: TimelinePosition | null;
  stateSegments: TimelineStateSegment[];
  deadlineMarker: TimelineMarker | null;
  completionMarker: TimelineMarker | null;
  overdue: boolean;
  openEnded: boolean;
  historyIncomplete: boolean;
}

export interface TimelineUnscheduledItem {
  itemId: string;
  title: string;
  status: ItemStatus;
  eventKind: EventKind | null;
  reason: string;
}

export interface TimelineData {
  scale: TimelineScale;
  rangeStart: string;
  rangeEnd: string;
  today: string;
  entries: TimelineEntry[];
  unscheduled: TimelineUnscheduledItem[];
  totalVisibleCount: number;
  totalUnscheduledCount: number;
  truncated: boolean;
  skippedInvalidCount: number;
  ticks: { label: string; position: number; weekend: boolean }[];
  todayPosition: number | null;
}
