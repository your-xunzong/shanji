<script lang="ts">
  import { onMount, tick } from 'svelte';
  import { isTauri } from '@tauri-apps/api/core';
  import ItemCard from '../components/ItemCard.svelte';
  import OnboardingDialog from '../components/OnboardingDialog.svelte';
  import SettingsPanel from '../components/SettingsPanel.svelte';
  import OrganizerPanel from '../components/OrganizerPanel.svelte';
  import ExportPanel from '../components/ExportPanel.svelte';
  import ReminderCenter from '../components/ReminderCenter.svelte';
  import RepositoryPanel from '../components/RepositoryPanel.svelte';
  import EmailRulesPanel from '../components/EmailRulesPanel.svelte';
  import TimelineView from '../components/TimelineView.svelte';
  import UpdatePanel from '../components/UpdatePanel.svelte';
  import '../workbench.css';
  import { api } from '../lib/api';
  import currentReleaseNotesRaw from '../release-notes/v0.13.1.md?raw';
  import {
    cancelAppUpdateDownload,
    checkForAppUpdate,
    clearPendingUpdate,
    downloadAppUpdate,
    installAppUpdate,
    parseUserFacingNotes,
    type AppUpdate,
    type UpdateDownloadProgress,
    type UpdatePhase,
  } from '../lib/updater';
  import type {
    Item,
    ItemFilter,
    AutostartStatus,
    NotificationStatus,
    SmtpStatus,
    OnboardingFinishInput,
    OnboardingStatus,
    Settings,
    UpdateSettingsInput,
    DataStatus,
    Category,
    Tag,
    TaxonomyInput,
    UpdateItemInput,
    ExportFilterInput,
    AppInfo,
    UpdateState,
  } from '../lib/types';

  let items: Item[] = [];
  let settings: Settings | null = null;
  let notificationStatus: NotificationStatus | null = null;
  let autostartStatus: AutostartStatus | null = null;
  let smtpStatus: SmtpStatus | null = null;
  let onboardingStatus: OnboardingStatus | null = null;
  let filter: ItemFilter = 'open';
  let loading = true;
  let error = '';
  let busyItemId = '';
  let settingsOpen = false;
  let onboardingOpen = false;
  let settingsSaving = false;
  let toast = '';
  let dataStatus: DataStatus | null = null;
  let categories: Category[] = [];
  let tags: Tag[] = [];
  let appInfo: AppInfo | null = null;
  let organizerOpen = false;
  let exporting = false;
  let exportOpen = false;
  let undoDeletedItem: Item | null = null;
  let toastTimer: ReturnType<typeof setTimeout> | undefined;
  let unlistenItems: (() => void) | undefined;
  let unlistenNotification: (() => void) | undefined;
  let unlistenReminderCenter: (() => void) | undefined;
  let unlistenOpenReminderCenter: (() => void) | undefined;
  let pendingReminders: Item[] = [];
  let reminderCenterOpen = false;
  let unlistenClassification: (() => void) | undefined;
  let pendingTypeCount = 0;
  let editingItemId = '';
  let viewMode: 'items' | 'timeline' = 'items';
  let repositoryOpen = false;
  let emailRulesOpen = false;
  let updateState: UpdateState | null = null;
  let availableUpdate: AppUpdate | null = null;
  let updatePanelOpen = false;
  let updatePhase: UpdatePhase = 'checking';
  let updateError = '';
  let updateProgress: UpdateDownloadProgress = { downloadedBytes: 0, totalBytes: null, percent: null, finished: false };
  let updateCheckTimer: ReturnType<typeof setTimeout> | undefined;
  let unlistenOpenUpdate: (() => void) | undefined;
  let cancellingUpdateDownload = false;

  const currentReleaseNotes = parseUserFacingNotes(currentReleaseNotesRaw);

  const navItems: { id: ItemFilter; label: string; marker: string }[] = [
    { id: 'open', label: '待处理', marker: '○' },
    { id: 'today', label: '今天', marker: '⌁' },
    { id: 'overdue', label: '已逾期', marker: '!' },
    { id: 'done', label: '已完成', marker: '✓' },
    { id: 'deleted', label: '回收站', marker: '×' },
    { id: 'all', label: '全部事项', marker: '·' },
  ];

  $: urgentCount = items.filter(
    (item) => item.completionPolicy === 'MUST_COMPLETE_TODAY' && item.status === 'OPEN',
  ).length;
  $: updateSnoozed = Boolean(
    availableUpdate
      && updateState?.snoozedVersion === availableUpdate.version
      && updateState.snoozedUntil
      && new Date(updateState.snoozedUntil).getTime() > Date.now(),
  );
  $: showUpdateBanner = Boolean(availableUpdate && !updateSnoozed);

  onMount(() => {
    void initialize();
    if (isTauri()) {
      void import('@tauri-apps/api/event').then(async ({ listen }) => {
        unlistenItems = await listen('items_changed', () => {
          void loadItems();
          void loadPendingReminders();
        });
        unlistenNotification = await listen<string>('notification_opened', ({ payload }) => {
          void revealNotificationItem(payload);
        });
        unlistenReminderCenter = await listen('reminder_center_changed', () => void loadPendingReminders());
        unlistenOpenReminderCenter = await listen('open_reminder_center', () => {
          reminderCenterOpen = true;
          void loadPendingReminders();
        });
        unlistenClassification = await listen<string>('classification_opened', ({ payload }) => {
          void revealClassification(payload);
        });
        unlistenOpenUpdate = await listen('open_update', () => {
          if (availableUpdate) {
            updatePhase = 'available';
            updatePanelOpen = true;
          } else {
            void checkUpdates(true);
          }
        });
      });
    }

    const onFocus = () => void loadItems();
    window.addEventListener('focus', onFocus);
    return () => {
      window.removeEventListener('focus', onFocus);
      unlistenItems?.();
      unlistenNotification?.();
      unlistenReminderCenter?.();
      unlistenOpenReminderCenter?.();
      unlistenClassification?.();
      unlistenOpenUpdate?.();
      if (updateCheckTimer) clearTimeout(updateCheckTimer);
      void clearPendingUpdate();
      if (toastTimer) clearTimeout(toastTimer);
    };
  });

  async function initialize(): Promise<void> {
    loading = true;
    error = '';
    try {
      const [
        loadedAppInfo,
        loadedSettings,
        loadedNotificationStatus,
        loadedAutostartStatus,
        loadedOnboardingStatus,
        ,
        warning,
        loadedCategories,
        loadedTags,
        loadedPendingReminders,
        loadedUpdateState,
      ] = await Promise.all([
        api.getAppInfo(),
        api.getSettings(),
        api.getNotificationStatus(),
        api.getAutostartStatus(),
        api.getOnboardingStatus(),
        loadItems(),
        api.getSystemWarning(),
        api.listCategories(),
        api.listTags(),
        api.listPendingReminders(),
        api.getUpdateState(),
      ]);
      appInfo = loadedAppInfo;
      if (loadedAutostartStatus.available) {
        loadedSettings.autostartEnabled = loadedAutostartStatus.enabled;
      }
      settings = loadedSettings;
      notificationStatus = loadedNotificationStatus;
      autostartStatus = loadedAutostartStatus;
      onboardingStatus = loadedOnboardingStatus;
      onboardingOpen = loadedOnboardingStatus.required;
      categories = loadedCategories;
      tags = loadedTags;
      pendingReminders = loadedPendingReminders;
      updateState = loadedUpdateState;
      if (loadedUpdateState.showCurrentReleaseNotes && !loadedOnboardingStatus.required) {
        updatePhase = 'current';
        updatePanelOpen = true;
      }
      scheduleAutomaticUpdateCheck(loadedUpdateState);
      if (warning) error = warning;
    } catch (cause) {
      error = readableError(cause, '事项暂时无法读取。闪记没有修改记录，请重试。');
    } finally {
      loading = false;
    }
  }

  function readableError(cause: unknown, fallback: string): string {
    if (fallback) return fallback;
    const detail = typeof cause === 'string' ? cause : cause instanceof Error ? cause.message : '';
    if (detail.includes('快捷键')) return '快捷键没有更改。请换一组组合键后重试。';
    if (detail.includes('开机启动')) return '开机启动设置没有保存。请在后台设置中重试。';
    return '';
  }

  function scheduleAutomaticUpdateCheck(state: UpdateState): void {
    if (updateCheckTimer) clearTimeout(updateCheckTimer);
    if (!state.shouldAutoCheck) return;
    updateCheckTimer = setTimeout(() => void checkUpdates(false), 30_000);
  }

  function safeUpdateError(cause: unknown, fallback: string): string {
    const message = cause instanceof Error ? cause.message : typeof cause === 'string' ? cause : '';
    if (message.includes('来源无法验证') || message.includes('签名') || message.includes('无法验证')) {
      return '更新无法验证，当前版本和本机数据没有改变。请重新检查或打开官方发布页。';
    }
    if (message.includes('下载已取消')) return '下载已取消，当前版本和本机数据没有改变。';
    return fallback;
  }

  async function checkUpdates(manual: boolean): Promise<void> {
    if (updatePhase === 'downloading' || updatePhase === 'installing') return;
    updateError = '';
    if (manual) {
      updatePhase = 'checking';
      updatePanelOpen = true;
      settingsOpen = false;
    }
    try {
      const update = await checkForAppUpdate();
      updateState = await api.recordUpdateCheck(update ? 'UPDATE_AVAILABLE' : 'UP_TO_DATE');
      availableUpdate = update;
      if (!update) {
        await api.setTrayUpdate(null);
        if (manual) updatePhase = 'upToDate';
        return;
      }

      await api.setTrayUpdate(update.version);
      const snoozed = updateState.snoozedVersion === update.version
        && Boolean(updateState.snoozedUntil)
        && new Date(updateState.snoozedUntil as string).getTime() > Date.now();
      if (!snoozed && updateState.lastNotifiedVersion !== update.version) {
        try {
          updateState = await api.notifyUpdateAvailable(update.version);
        } catch {
          updateState = await api.getUpdateState();
        }
      }
      if (manual) updatePhase = 'available';
    } catch (cause) {
      try {
        updateState = await api.recordUpdateCheck('FAILED');
      } catch {
        // The visible action remains recoverable even if writing the check timestamp failed.
      }
      if (manual) {
        updateError = safeUpdateError(cause, '暂时无法检查更新；事项和当前版本未受影响。');
        updatePhase = 'failed';
      }
    }
  }

  async function enableAutomaticUpdates(): Promise<void> {
    try {
      updateState = await api.setAutoUpdateEnabled(true);
      await checkUpdates(true);
    } catch {
      error = '自动检查设置没有保存。闪记仍保持离线，请稍后重试。';
    }
  }

  async function keepUpdatesOffline(): Promise<void> {
    try {
      updateState = await api.dismissUpdatePermission();
    } catch {
      error = '暂时无法保存更新选择；闪记没有开始联网。';
    }
  }

  async function setAutomaticUpdates(enabled: boolean): Promise<UpdateState> {
    updateState = await api.setAutoUpdateEnabled(enabled);
    scheduleAutomaticUpdateCheck(updateState);
    return updateState;
  }

  async function downloadUpdate(): Promise<void> {
    if (!availableUpdate || appInfo?.updateInstallMode !== 'AUTOMATIC') return;
    updatePhase = 'downloading';
    updateError = '';
    updateProgress = { downloadedBytes: 0, totalBytes: null, percent: null, finished: false };
    cancellingUpdateDownload = false;
    try {
      await downloadAppUpdate((progress) => (updateProgress = progress));
      updatePhase = 'downloaded';
    } catch (cause) {
      if (cancellingUpdateDownload) {
        updatePhase = 'cancelled';
      } else {
        updateError = safeUpdateError(cause, '更新包没有下载完成，当前版本和本机数据没有改变。');
        updatePhase = 'failed';
      }
    }
  }

  async function cancelUpdateDownload(): Promise<void> {
    cancellingUpdateDownload = true;
    await cancelAppUpdateDownload();
    updatePhase = 'cancelled';
  }

  async function installUpdate(): Promise<void> {
    updatePhase = 'installing';
    updateError = '';
    try {
      await installAppUpdate();
    } catch (cause) {
      updateError = safeUpdateError(cause, '更新没有安装，当前版本和本机数据仍然可用。请重试或手动下载安装包。');
      updatePhase = 'failed';
    }
  }

  async function snoozeAvailableUpdate(): Promise<void> {
    if (!availableUpdate) return;
    updateState = await api.snoozeUpdate(availableUpdate.version);
    updatePanelOpen = false;
  }

  async function openReleasePage(): Promise<void> {
    await api.openReleasePage(availableUpdate?.version ?? appInfo?.version ?? null);
  }

  async function closeUpdatePanel(): Promise<void> {
    if (updatePhase === 'downloading' || updatePhase === 'installing') return;
    if (updatePhase === 'current' && appInfo) {
      try {
        updateState = await api.markReleaseNotesSeen(appInfo.version);
      } catch {
        // Closing release notes never blocks the rest of the application.
      }
    }
    updatePanelOpen = false;
    await tick();
    document.querySelector<HTMLElement>('.update-available-banner, .settings-entry')?.focus();
  }

  function showCurrentReleaseNotes(): void {
    settingsOpen = false;
    updatePhase = 'current';
    updatePanelOpen = true;
  }

  async function loadItems(): Promise<void> {
    const loaded = await api.listItems(filter);
    items = loaded;
    const openItems = filter === 'open' ? loaded : await api.listItems('open');
    pendingTypeCount = openItems.filter((item) => !item.eventKind).length;
  }

  async function loadPendingReminders(): Promise<void> {
    pendingReminders = await api.listPendingReminders();
  }

  async function completePendingReminder(item: Item): Promise<void> {
    await completeItem(item, true);
    await loadPendingReminders();
  }

  async function snoozePendingReminder(item: Item, minutes: number): Promise<void> {
    busyItemId = item.id;
    try {
      await api.snoozeItem(item.id, minutes);
      await Promise.all([loadItems(), loadPendingReminders()]);
      showToast(`已在 ${minutes} 分钟后再次提醒`);
    } finally {
      busyItemId = '';
    }
  }

  async function acknowledgePendingReminder(item: Item): Promise<void> {
    busyItemId = item.id;
    try {
      await api.acknowledgeReminder(item.id);
      await loadPendingReminders();
      showToast('已确认看到，本次提醒已从中心移除');
    } finally {
      busyItemId = '';
    }
  }

  async function openPendingReminder(item: Item): Promise<void> {
    reminderCenterOpen = false;
    await revealNotificationItem(item.id);
  }

  async function selectFilter(next: ItemFilter): Promise<void> {
    viewMode = 'items';
    filter = next;
    loading = true;
    error = '';
    try {
      await loadItems();
    } catch (cause) {
      error = readableError(cause, '事项列表未能刷新，记录没有被修改。请重试。');
    } finally {
      loading = false;
    }
  }

  async function openTimelineItem(id: string): Promise<void> {
    viewMode = 'items';
    filter = 'all';
    editingItemId = id;
    loading = true;
    error = '';
    try {
      await loadItems();
      await tick();
      document.querySelector<HTMLElement>(`[data-item-id="${CSS.escape(id)}"]`)?.scrollIntoView({ block: 'center' });
    } catch (cause) {
      error = readableError(cause, '事项暂时无法打开，内容没有被修改。请重试。');
    } finally {
      loading = false;
    }
  }

  async function completeItem(item: Item, completed: boolean): Promise<void> {
    if (completed && (item.eventKind === 'MONTHLY' || item.eventKind === 'YEARLY')
      && !window.confirm(`结束“${item.title}”的整个系列吗？以后将不再生成提醒。`)) return;
    busyItemId = item.id;
    error = '';
    try {
      await api.setItemCompleted(item.id, completed);
      await loadItems();
      showToast(completed ? '事项已完成，提醒已停止' : '事项已恢复');
    } catch (cause) {
      error = readableError(cause, '状态没有保存，事项保持原样。');
    } finally {
      busyItemId = '';
    }
  }

  async function pauseItem(item: Item, paused: boolean): Promise<void> {
    busyItemId = item.id;
    error = '';
    try {
      await api.setReminderPaused(item.id, paused);
      await loadItems();
      showToast(paused ? '持续提醒已暂停' : '持续提醒已恢复');
    } catch (cause) {
      error = readableError(cause, '提醒状态没有保存。');
    } finally {
      busyItemId = '';
    }
  }

  async function rescheduleItem(item: Item, dueAt: string): Promise<void> {
    busyItemId = item.id;
    error = '';
    try {
      await api.rescheduleItem(item.id, dueAt);
      await loadItems();
      showToast('时间已调整，提醒计划已更新');
    } catch (cause) {
      error = readableError(cause, '时间没有保存，原提醒计划保持不变。');
      throw cause;
    } finally {
      busyItemId = '';
    }
  }

  async function updateItem(item: Item, input: UpdateItemInput): Promise<void> {
    busyItemId = item.id;
    error = '';
    try {
      await api.updateItem(item.id, input);
      editingItemId = '';
      await loadItems();
      showToast('事项修改已保存');
    } catch (cause) {
      error = readableError(cause, '修改没有保存，事项保持原样。');
      throw cause;
    } finally {
      busyItemId = '';
    }
  }

  async function deleteItem(item: Item, deleted: boolean): Promise<void> {
    if (deleted && !window.confirm(`将“${item.title}”移到回收站吗？提醒会立即停止。`)) return;
    busyItemId = item.id;
    try {
      await api.setItemDeleted(item.id, deleted);
      await loadItems();
      if (deleted) {
        undoDeletedItem = item;
        showToast('已移到回收站，提醒已停止', 6000);
      } else {
        undoDeletedItem = null;
        showToast('事项已恢复');
      }
    } catch (cause) {
      error = readableError(cause, '操作没有保存，事项保持原样。');
    } finally {
      busyItemId = '';
    }
  }

  async function undoDelete(): Promise<void> {
    if (!undoDeletedItem) return;
    const item = undoDeletedItem;
    busyItemId = item.id;
    error = '';
    try {
      await api.setItemDeleted(item.id, false);
      undoDeletedItem = null;
      await loadItems();
      showToast('已撤销删除，事项和提醒已恢复');
    } catch (cause) {
      error = readableError(cause, '暂时无法撤销，请到回收站恢复事项。');
    } finally {
      busyItemId = '';
    }
  }

  async function permanentlyDeleteItem(item: Item): Promise<void> {
    if (!window.confirm(`永久删除“${item.title}”吗？此操作无法撤销。`)) return;
    busyItemId = item.id;
    try {
      await api.permanentlyDeleteItem(item.id);
      await loadItems();
      showToast('事项已永久删除');
    } catch (cause) {
      error = readableError(cause, '事项没有删除，请重试。');
    } finally {
      busyItemId = '';
    }
  }

  async function refreshTaxonomies(): Promise<void> {
    [categories, tags] = await Promise.all([api.listCategories(), api.listTags()]);
    await loadItems();
  }

  async function createCategory(input: TaxonomyInput): Promise<void> {
    await api.createCategory(input);
    await refreshTaxonomies();
  }

  async function updateCategory(id: string, input: TaxonomyInput): Promise<void> {
    await api.updateCategory(id, input);
    await refreshTaxonomies();
  }

  async function deleteCategory(id: string, reassignTo: string | null): Promise<void> {
    await api.deleteCategory(id, reassignTo);
    await refreshTaxonomies();
  }

  async function moveCategory(id: string, direction: 'up' | 'down'): Promise<void> {
    await api.moveCategory(id, direction);
    await refreshTaxonomies();
  }

  async function createTag(input: TaxonomyInput): Promise<void> {
    await api.createTag(input);
    await refreshTaxonomies();
  }

  async function updateTag(id: string, input: TaxonomyInput): Promise<void> {
    await api.updateTag(id, input);
    await refreshTaxonomies();
  }

  async function deleteTag(id: string): Promise<void> {
    await api.deleteTag(id);
    await refreshTaxonomies();
  }

  async function moveTag(id: string, direction: 'up' | 'down'): Promise<void> {
    await api.moveTag(id, direction);
    await refreshTaxonomies();
  }

  async function completeOccurrence(item: Item): Promise<void> {
    busyItemId = item.id;
    try {
      await api.completeSeriesOccurrence(item.id);
      await Promise.all([loadItems(), loadPendingReminders()]);
      showToast('本次已完成，后续周期仍会继续');
    } catch (cause) {
      error = readableError(cause, '本次完成状态没有保存。');
    } finally {
      busyItemId = '';
    }
  }

  async function exportExcel(filter: ExportFilterInput): Promise<void> {
    if (exporting) return;
    exporting = true;
    error = '';
    try {
      const defaultName = `闪记事项-${new Date().toISOString().slice(0, 10)}.xlsx`;
      let path = defaultName;
      if (isTauri()) {
        const { save } = await import('@tauri-apps/plugin-dialog');
        const selected = await save({ defaultPath: defaultName, filters: [{ name: 'Excel 报表', extensions: ['xlsx'] }] });
        if (!selected) return;
        path = selected.endsWith('.xlsx') ? selected : `${selected}.xlsx`;
      }
      const result = await api.exportExcel(path, filter);
      exportOpen = false;
      showToast(`已导出 ${result.itemCount} 项：${result.path}`, 5000);
    } catch (cause) {
      error = readableError(cause, 'Excel 报表没有生成，请重新选择保存位置。');
    } finally {
      exporting = false;
    }
  }

  async function saveSettings(input: UpdateSettingsInput): Promise<void> {
    settingsSaving = true;
    error = '';
    try {
      settings = await api.updateSettings(input);
      autostartStatus = await api.getAutostartStatus();
      if (autostartStatus.available) settings.autostartEnabled = autostartStatus.enabled;
      settingsOpen = false;
      await loadItems();
      showToast('设置已保存');
    } catch (cause) {
      throw new Error(readableError(cause, '设置没有保存，请检查输入后重试。'));
    } finally {
      settingsSaving = false;
    }
  }

  async function openSettings(): Promise<void> {
    try {
      const [latestAutostart, latestNotifications, latestDataStatus, latestSmtp] = await Promise.all([
        api.getAutostartStatus(),
        api.getNotificationStatus(),
        api.getDataStatus(),
        api.getSmtpStatus(),
      ]);
      autostartStatus = latestAutostart;
      notificationStatus = latestNotifications;
      dataStatus = latestDataStatus;
      smtpStatus = latestSmtp;
      if (settings && latestAutostart.available) {
        settings = { ...settings, autostartEnabled: latestAutostart.enabled };
      }
      settingsOpen = true;
    } catch (cause) {
      error = readableError(cause, '无法读取系统设置状态。');
    }
  }

  async function openEmailRules(): Promise<void> {
    try {
      smtpStatus = await api.getSmtpStatus();
      emailRulesOpen = true;
    } catch (cause) {
      error = readableError(cause, '邮件提醒状态暂时无法读取。');
    }
  }

  async function refreshDataStatus(): Promise<DataStatus> {
    dataStatus = await api.getDataStatus();
    return dataStatus;
  }

  async function refreshSmtpStatus(): Promise<SmtpStatus> {
    smtpStatus = await api.getSmtpStatus();
    return smtpStatus;
  }

  async function testSmtp(): Promise<string> {
    const message = await api.testSmtp();
    smtpStatus = await api.getSmtpStatus();
    return message;
  }

  async function createDataBackup() {
    const backup = await api.createDataBackup();
    showToast('数据备份已完成');
    return backup;
  }

  async function openDataDirectory(): Promise<void> {
    await api.openDataDirectory();
  }

  async function restoreDatabase(path: string): Promise<void> {
    await api.restoreDatabase(path);
  }

  function reopenOnboarding(): void {
    settingsOpen = false;
    onboardingOpen = true;
  }

  async function finishOnboarding(input: OnboardingFinishInput): Promise<void> {
    if (!settings || !notificationStatus) return;
    if (input.enablePortableNotifications && !notificationStatus.canNotify) {
      notificationStatus = await api.registerPortableNotifications();
    }
    try {
      settings = await api.updateSettings({
        ...settings,
        defaultDueTime: input.defaultDueTime,
        repeatDefaultTimes: input.repeatDefaultTimes,
        autostartEnabled: input.autostartEnabled,
        globalShortcut: input.globalShortcut,
        updateExistingDefaultItems: false,
      });
    } catch (cause) {
      const message = readableError(cause, '');
      if (message.includes('快捷键') || message.includes('开机启动')) throw new Error(message);
      throw new Error('时间设置没有保存，原提醒时间仍然有效，请重试。');
    }
    autostartStatus = await api.getAutostartStatus();
    if (autostartStatus.available) settings.autostartEnabled = autostartStatus.enabled;
    onboardingStatus = await api.completeOnboarding();
    onboardingOpen = false;
    showToast('首次设置已完成');
  }

  async function skipOnboarding(): Promise<void> {
    if (onboardingStatus?.required) onboardingStatus = await api.completeOnboarding();
    onboardingOpen = false;
  }

  async function testNotification(): Promise<void> {
    await api.createNotificationTestItem();
    await loadItems();
    setTimeout(() => void refreshNotificationStatus(), 12_000);
  }

  async function testReminderMode(mode: 'persistent' | 'overlay'): Promise<string> {
    const message = await api.testReminderMode(mode);
    await Promise.all([loadItems(), loadPendingReminders()]);
    return message;
  }

  function openReminderCenterFromSettings(): void {
    settingsOpen = false;
    reminderCenterOpen = true;
    void loadPendingReminders();
  }

  async function refreshNotificationStatus(): Promise<void> {
    try {
      notificationStatus = await api.getNotificationStatus();
    } catch (cause) {
      error = readableError(cause, '无法刷新系统通知状态。');
    }
  }

  async function registerNotifications(): Promise<void> {
    notificationStatus = await api.registerPortableNotifications();
    showToast('便携版系统通知已启用');
  }

  async function unregisterNotifications(): Promise<void> {
    notificationStatus = await api.unregisterPortableNotifications();
    showToast('便携版系统通知注册已撤销');
  }

  async function revealNotificationItem(id: string): Promise<void> {
    try {
      filter = 'all';
      await loadItems();
      await tick();
      document.querySelector<HTMLElement>(`[data-item-id="${CSS.escape(id)}"]`)?.scrollIntoView({
        behavior: 'smooth',
        block: 'center',
      });
    } catch (cause) {
      error = readableError(cause, '已打开闪记，但对应事项暂时无法定位。');
    }
  }

  async function revealClassification(id: string): Promise<void> {
    try {
      filter = 'all';
      editingItemId = id;
      await loadItems();
      await tick();
      if (id) {
        document.querySelector<HTMLElement>(`[data-item-id="${CSS.escape(id)}"]`)?.scrollIntoView({
          behavior: 'smooth',
          block: 'center',
        });
      }
    } catch (cause) {
      error = readableError(cause, '待选择事件类型的记录暂时无法打开。');
    }
  }

  function showToast(message: string, duration = 2600): void {
    if (toastTimer) clearTimeout(toastTimer);
    toast = message;
    toastTimer = setTimeout(() => {
      if (toast === message) {
        toast = '';
        undoDeletedItem = null;
      }
    }, duration);
  }

  const dateLabel = new Intl.DateTimeFormat('zh-CN', {
    month: 'long',
    day: 'numeric',
    weekday: 'long',
  }).format(new Date());
</script>

<svelte:head>
  <title>闪记 · 事项</title>
</svelte:head>

<div class="app-shell workspace">
  <aside class="sidebar">
    <div class="brand-block">
      <span class="brand-mark large" aria-hidden="true"></span>
      <div>
        <strong>闪记</strong>
        <small>写下，再回来</small>
      </div>
    </div>

    <nav aria-label="事项筛选">
      <p class="nav-label">事项</p>
      {#each navItems as navItem}
        <button
          class="nav-item"
          class:active={viewMode === 'items' && filter === navItem.id}
          aria-current={viewMode === 'items' && filter === navItem.id ? 'page' : undefined}
          aria-label={navItem.label}
          title={navItem.label}
          on:click={() => selectFilter(navItem.id)}
        >
          <span class="nav-marker">{navItem.marker}</span>
          <span>{navItem.label}</span>
          {#if navItem.id === 'open' && items.length > 0}<span class="nav-count">{items.length}</span>{/if}
        </button>
      {/each}
      <button class="nav-item" class:active={viewMode === 'timeline'} aria-current={viewMode === 'timeline' ? 'page' : undefined} aria-label="事项甘特图" title="事项甘特图" on:click={() => (viewMode = 'timeline')}>
        <span class="nav-marker">━</span><span>事项甘特图</span>
      </button>
    </nav>

    <div class="sidebar-rule"></div>
    <button class="nav-item settings-entry" aria-label="后台设置" title="后台设置" on:click={() => void openSettings()}>
      <span class="nav-marker">⌘</span>
      <span>后台设置</span>
    </button>
    <button class="nav-item settings-entry" aria-label="整理方式" title="整理方式" on:click={() => (organizerOpen = true)}>
      <span class="nav-marker">◇</span><span>整理方式</span>
    </button>
    <button class="nav-item settings-entry" aria-label="提醒中心" title="提醒中心" on:click={() => (reminderCenterOpen = true)}>
      <span class="nav-marker">!</span><span>提醒中心</span>
      {#if pendingReminders.length > 0}<span class="nav-count">{pendingReminders.length}</span>{/if}
    </button>
    <button class="nav-item settings-entry" aria-label="邮件提醒" title="邮件提醒" on:click={() => void openEmailRules()}>
      <span class="nav-marker">↗</span><span>邮件提醒</span>
    </button>
    <button class="nav-item settings-entry" aria-label="个人仓库" title="个人仓库" on:click={() => (repositoryOpen = true)}>
      <span class="nav-marker">◇</span><span>个人仓库</span>
    </button>

    <div class="shortcut-note">
      <kbd>{navigator.platform.includes('Mac') ? '⌘' : 'Ctrl'}</kbd>
      <kbd>Shift</kbd>
      <kbd>Space</kbd>
      <span>快速记录</span>
    </div>
  </aside>

  <main class="dashboard">
    {#if viewMode === 'timeline'}
      <TimelineView {categories} {tags} onOpenItem={openTimelineItem} onBackToItems={() => selectFilter('open')} />
    {:else}
    <header class="dashboard-header">
      <div>
        <p class="eyebrow">{dateLabel}</p>
        <h1>{navItems.find((entry) => entry.id === filter)?.label}</h1>
      </div>
      <div class="header-actions">
        {#if urgentCount > 0}
          <span class="urgent-summary"><span></span>{urgentCount} 项今日必做</span>
        {/if}
        <button class="secondary-button export-button" disabled={exporting} on:click={() => (exportOpen = true)}>{exporting ? '正在导出…' : '导出 Excel'}</button>
        <button class="new-item-button" on:click={() => api.showCapture()}>
          <span aria-hidden="true">＋</span> 快速记录
        </button>
      </div>
    </header>

    {#if updateState && !updateState.permissionPrompted && !onboardingOpen}
      <section class="update-permission-banner" aria-labelledby="update-permission-title">
        <span class="update-banner-mark" aria-hidden="true">↗</span>
        <span>
          <strong id="update-permission-title">获取新版本提醒</strong>
          <small>允许闪记每天最多连接 GitHub 一次；不会上传事项、备注或本地路径。</small>
        </span>
        <button class="text-button" on:click={keepUpdatesOffline}>保持离线</button>
        <button class="secondary-button" on:click={enableAutomaticUpdates}>启用并检查</button>
      </section>
    {/if}

    {#if showUpdateBanner && availableUpdate}
      <button class="update-available-banner" on:click={() => { updatePhase = 'available'; updatePanelOpen = true; }}>
        <span class="update-banner-mark" aria-hidden="true">↗</span>
        <span><strong>新版本 v{availableUpdate.version} 可用</strong><small>{availableUpdate.notes[0]}</small></span>
        <b>查看更新 →</b>
      </button>
    {/if}

    {#if error}
      <div class="inline-error" role="alert">
        <span>{error}</span>
        <button on:click={initialize}>重试</button>
      </div>
    {/if}

    {#if pendingTypeCount > 0}
      <button class="classification-banner" on:click={() => revealClassification('')}>
        <span><strong>{pendingTypeCount} 条记录待选择事件类型</strong><small>记录已经保存，可以在后台集中整理。</small></span>
        <b>现在处理 →</b>
      </button>
    {/if}

    <section class="item-stream" aria-live="polite" aria-busy={loading}>
      {#if loading}
        <div class="loading-list" aria-label="正在读取事项">
          <span></span><span></span><span></span>
        </div>
      {:else if items.length === 0 && !error}
        <div class="empty-state">
          <div class="empty-rail" aria-hidden="true"></div>
          <p class="eyebrow">这里暂时是空的</p>
          <h2>{filter === 'done' ? '还没有完成记录' : '当前列表没有事项'}</h2>
          <p>按全局快捷键打开快速输入，保存后会自动回到当前工作。</p>
          <button class="primary-button" on:click={() => api.showCapture()}>写第一条</button>
        </div>
      {:else if items.length > 0}
        <div class="stream-heading">
          <span>{items.length} 项</span>
          <button on:click={() => selectFilter(filter)}>刷新</button>
        </div>
        {#each items as item (item.id)}
          <ItemCard
            {item}
            busy={busyItemId === item.id}
            onComplete={completeItem}
            onPause={pauseItem}
            onReschedule={rescheduleItem}
            {categories}
            {tags}
            repeatDefaultTimes={settings?.repeatDefaultTimes ?? ['10:00', '17:00']}
            onUpdate={updateItem}
            onDelete={deleteItem}
            onPermanentDelete={permanentlyDeleteItem}
            onCompleteOccurrence={completeOccurrence}
            openEditor={editingItemId === item.id}
          />
        {/each}
      {/if}
    </section>
    {/if}
  </main>

  {#if settingsOpen && settings && notificationStatus && autostartStatus && smtpStatus}
    <SettingsPanel
      {settings}
      saving={settingsSaving}
      onClose={() => (settingsOpen = false)}
      onSave={saveSettings}
      onTestNotification={testNotification}
      onTestReminderMode={testReminderMode}
      pendingReminderCount={pendingReminders.length}
      onOpenReminderCenter={openReminderCenterFromSettings}
      {smtpStatus}
      onTestSmtp={testSmtp}
      onRefreshSmtpStatus={refreshSmtpStatus}
      onOpenEmailRules={() => {
        settingsOpen = false;
        void openEmailRules();
      }}
      {notificationStatus}
      onRegisterNotifications={registerNotifications}
      onUnregisterNotifications={unregisterNotifications}
      {autostartStatus}
      onOpenOnboarding={reopenOnboarding}
      {dataStatus}
      onRefreshData={refreshDataStatus}
      onCreateBackup={createDataBackup}
      onOpenDataDirectory={openDataDirectory}
      onRestoreDatabase={restoreDatabase}
      {appInfo}
      {updateState}
      updateChecking={updatePhase === 'checking' && updatePanelOpen}
      onSetAutoUpdate={setAutomaticUpdates}
      onCheckForUpdates={() => checkUpdates(true)}
      onViewReleaseNotes={showCurrentReleaseNotes}
    />
  {/if}

  {#if emailRulesOpen && settings && smtpStatus}
    <EmailRulesPanel
      {categories}
      {tags}
      {settings}
      {smtpStatus}
      onClose={() => (emailRulesOpen = false)}
      onOpenSettings={() => {
        emailRulesOpen = false;
        void openSettings();
      }}
    />
  {/if}

  {#if repositoryOpen}
    <RepositoryPanel
      onClose={() => (repositoryOpen = false)}
      onItemsChanged={async () => {
        await Promise.all([loadItems(), refreshTaxonomies(), loadPendingReminders()]);
        showToast('个人仓库同步完成');
      }}
      onShowAllItems={async () => {
        repositoryOpen = false;
        await selectFilter('all');
      }}
    />
  {/if}

  {#if organizerOpen}
    <OrganizerPanel
      {categories}
      {tags}
      onClose={() => (organizerOpen = false)}
      onCreateCategory={createCategory}
      onUpdateCategory={updateCategory}
      onDeleteCategory={deleteCategory}
      onMoveCategory={moveCategory}
      onCreateTag={createTag}
      onUpdateTag={updateTag}
      onDeleteTag={deleteTag}
      onMoveTag={moveTag}
    />
  {/if}

  {#if exportOpen}
    <ExportPanel
      {categories}
      {tags}
      {exporting}
      onClose={() => (exportOpen = false)}
      onExport={exportExcel}
    />
  {/if}

  {#if reminderCenterOpen}
    <ReminderCenter
      items={pendingReminders}
      {busyItemId}
      onClose={() => (reminderCenterOpen = false)}
      onComplete={completePendingReminder}
      onSnooze={snoozePendingReminder}
      onAcknowledge={acknowledgePendingReminder}
      onOpen={openPendingReminder}
    />
  {/if}

  {#if onboardingOpen && settings && notificationStatus && autostartStatus && onboardingStatus}
    <OnboardingDialog
      {settings}
      {notificationStatus}
      {autostartStatus}
      firstRun={onboardingStatus.required}
      onFinish={finishOnboarding}
      onSkip={skipOnboarding}
    />
  {/if}

  {#if updatePanelOpen && appInfo}
    <UpdatePanel
      phase={updatePhase}
      update={availableUpdate}
      currentVersion={appInfo.version}
      currentNotes={currentReleaseNotes}
      progress={updateProgress}
      error={updateError}
      installMode={appInfo.updateInstallMode}
      onClose={() => void closeUpdatePanel()}
      onRetry={() => checkUpdates(true)}
      onDownload={downloadUpdate}
      onCancelDownload={cancelUpdateDownload}
      onInstall={installUpdate}
      onSnooze={snoozeAvailableUpdate}
      onOpenRelease={openReleasePage}
    />
  {/if}

  {#if toast}
    <div class="toast" role="status">
      <span>{toast}</span>
      {#if undoDeletedItem}<button on:click={undoDelete}>撤销</button>{/if}
    </div>
  {/if}
</div>
