<script lang="ts">
  import { onMount, tick } from 'svelte';
  import { isTauri } from '@tauri-apps/api/core';
  import ItemCard from '../components/ItemCard.svelte';
  import OnboardingDialog from '../components/OnboardingDialog.svelte';
  import SettingsPanel from '../components/SettingsPanel.svelte';
  import OrganizerPanel from '../components/OrganizerPanel.svelte';
  import ExportPanel from '../components/ExportPanel.svelte';
  import ReminderCenter from '../components/ReminderCenter.svelte';
  import { api } from '../lib/api';
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
  let emailRouteTagIds: string[] = [];
  let unlistenClassification: (() => void) | undefined;
  let pendingTypeCount = 0;
  let editingItemId = '';

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
      if (warning) error = warning;
    } catch (cause) {
      error = readableError(cause, '无法打开本地数据。请检查数据目录后重试。');
    } finally {
      loading = false;
    }
  }

  function readableError(cause: unknown, fallback: string): string {
    if (typeof cause === 'string' && cause) return cause;
    return cause instanceof Error && cause.message ? cause.message : fallback;
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
    filter = next;
    loading = true;
    try {
      await loadItems();
    } catch (cause) {
      error = readableError(cause, '事项列表刷新失败。');
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
      const [latestAutostart, latestNotifications, latestDataStatus, latestSmtp, latestEmailRoutes] = await Promise.all([
        api.getAutostartStatus(),
        api.getNotificationStatus(),
        api.getDataStatus(),
        api.getSmtpStatus(),
        api.listEmailRouteTagIds(),
      ]);
      autostartStatus = latestAutostart;
      notificationStatus = latestNotifications;
      dataStatus = latestDataStatus;
      smtpStatus = latestSmtp;
      emailRouteTagIds = latestEmailRoutes;
      if (settings && latestAutostart.available) {
        settings = { ...settings, autostartEnabled: latestAutostart.enabled };
      }
      settingsOpen = true;
    } catch (cause) {
      error = readableError(cause, '无法读取系统设置状态。');
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

  async function setEmailRoute(tagId: string, enabled: boolean): Promise<void> {
    await api.setTagEmailRoute(tagId, enabled);
    emailRouteTagIds = await api.listEmailRouteTagIds();
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
        autostartEnabled: input.autostartEnabled,
        globalShortcut: input.globalShortcut,
        updateExistingDefaultItems: false,
      });
    } catch (cause) {
      const message = readableError(cause, '');
      if (message.includes('快捷键') || message.includes('开机启动')) throw cause;
      throw new Error('默认提醒时间没有保存，原设置仍然有效，请重试。');
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

  async function testReminderMode(mode: 'standard' | 'persistent' | 'overlay' | 'repeat' | 'center'): Promise<string> {
    const message = await api.testReminderMode(mode);
    await Promise.all([loadItems(), loadPendingReminders()]);
    return message;
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

<div class="app-shell">
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
          class:active={filter === navItem.id}
          aria-current={filter === navItem.id ? 'page' : undefined}
          on:click={() => selectFilter(navItem.id)}
        >
          <span class="nav-marker">{navItem.marker}</span>
          <span>{navItem.label}</span>
          {#if navItem.id === 'open' && items.length > 0}<span class="nav-count">{items.length}</span>{/if}
        </button>
      {/each}
    </nav>

    <div class="sidebar-rule"></div>
    <button class="nav-item settings-entry" on:click={() => void openSettings()}>
      <span class="nav-marker">⌘</span>
      <span>后台设置</span>
    </button>
    <button class="nav-item settings-entry" on:click={() => (organizerOpen = true)}>
      <span class="nav-marker">◇</span><span>整理方式</span>
    </button>
    <button class="nav-item settings-entry" on:click={() => (reminderCenterOpen = true)}>
      <span class="nav-marker">!</span><span>提醒中心</span>
      {#if pendingReminders.length > 0}<span class="nav-count">{pendingReminders.length}</span>{/if}
    </button>

    <div class="shortcut-note">
      <kbd>{navigator.platform.includes('Mac') ? '⌘' : 'Ctrl'}</kbd>
      <kbd>Shift</kbd>
      <kbd>Space</kbd>
      <span>快速记录</span>
    </div>
  </aside>

  <main class="dashboard">
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
      {:else if items.length === 0}
        <div class="empty-state">
          <div class="empty-rail" aria-hidden="true"></div>
          <p class="eyebrow">这里暂时是空的</p>
          <h2>{filter === 'done' ? '还没有完成记录' : '想到什么，就在原地记下'}</h2>
          <p>按全局快捷键打开快速输入，保存后会自动回到当前工作。</p>
          <button class="primary-button" on:click={() => api.showCapture()}>写第一条</button>
        </div>
      {:else}
        <div class="stream-heading">
          <span>{items.length} 项</span>
          <button on:click={loadItems}>刷新</button>
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
            onUpdate={updateItem}
            onDelete={deleteItem}
            onPermanentDelete={permanentlyDeleteItem}
            onCompleteOccurrence={completeOccurrence}
            openEditor={editingItemId === item.id}
          />
        {/each}
      {/if}
    </section>
  </main>

  {#if settingsOpen && settings && notificationStatus && autostartStatus && smtpStatus}
    <SettingsPanel
      {settings}
      saving={settingsSaving}
      onClose={() => (settingsOpen = false)}
      onSave={saveSettings}
      onTestNotification={testNotification}
      onTestReminderMode={testReminderMode}
      {smtpStatus}
      {tags}
      {emailRouteTagIds}
      onSetEmailRoute={setEmailRoute}
      onTestSmtp={testSmtp}
      onRefreshSmtpStatus={refreshSmtpStatus}
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

  {#if toast}
    <div class="toast" role="status">
      <span>{toast}</span>
      {#if undoDeletedItem}<button on:click={undoDelete}>撤销</button>{/if}
    </div>
  {/if}
</div>
