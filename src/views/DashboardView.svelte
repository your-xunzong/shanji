<script lang="ts">
  import { onMount } from 'svelte';
  import { isTauri } from '@tauri-apps/api/core';
  import ItemCard from '../components/ItemCard.svelte';
  import SettingsPanel from '../components/SettingsPanel.svelte';
  import { api } from '../lib/api';
  import type { Item, ItemFilter, Settings, UpdateSettingsInput } from '../lib/types';

  let items: Item[] = [];
  let settings: Settings | null = null;
  let filter: ItemFilter = 'open';
  let loading = true;
  let error = '';
  let busyItemId = '';
  let settingsOpen = false;
  let settingsSaving = false;
  let toast = '';
  let unlisten: (() => void) | undefined;

  const navItems: { id: ItemFilter; label: string; marker: string }[] = [
    { id: 'open', label: '待处理', marker: '○' },
    { id: 'today', label: '今天', marker: '⌁' },
    { id: 'overdue', label: '已逾期', marker: '!' },
    { id: 'done', label: '已完成', marker: '✓' },
    { id: 'all', label: '全部事项', marker: '·' },
  ];

  $: urgentCount = items.filter(
    (item) => item.completionPolicy === 'MUST_COMPLETE_TODAY' && item.status === 'OPEN',
  ).length;

  onMount(() => {
    void initialize();
    if (isTauri()) {
      void import('@tauri-apps/api/event').then(async ({ listen }) => {
        unlisten = await listen('items_changed', () => void loadItems());
      });
    }

    const onFocus = () => void loadItems();
    window.addEventListener('focus', onFocus);
    return () => {
      window.removeEventListener('focus', onFocus);
      unlisten?.();
    };
  });

  async function initialize(): Promise<void> {
    loading = true;
    error = '';
    try {
      const [loadedSettings, , warning] = await Promise.all([
        api.getSettings(),
        loadItems(),
        api.getSystemWarning(),
      ]);
      settings = loadedSettings;
      if (warning) error = warning;
    } catch (cause) {
      error = readableError(cause, '无法打开本地数据。请检查数据目录后重试。');
    } finally {
      loading = false;
    }
  }

  function readableError(cause: unknown, fallback: string): string {
    return cause instanceof Error && cause.message ? cause.message : fallback;
  }

  async function loadItems(): Promise<void> {
    items = await api.listItems(filter);
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

  async function saveSettings(input: UpdateSettingsInput): Promise<void> {
    settingsSaving = true;
    error = '';
    try {
      settings = await api.updateSettings(input);
      settingsOpen = false;
      await loadItems();
      showToast('设置已保存');
    } catch (cause) {
      error = readableError(cause, '设置没有保存，请检查输入后重试。');
    } finally {
      settingsSaving = false;
    }
  }

  async function testNotification(): Promise<void> {
    await api.createNotificationTestItem();
    await loadItems();
  }

  function showToast(message: string): void {
    toast = message;
    setTimeout(() => {
      if (toast === message) toast = '';
    }, 2600);
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
    <button class="nav-item settings-entry" on:click={() => (settingsOpen = true)}>
      <span class="nav-marker">⌘</span>
      <span>后台设置</span>
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
          />
        {/each}
      {/if}
    </section>
  </main>

  {#if settingsOpen && settings}
    <SettingsPanel
      {settings}
      saving={settingsSaving}
      onClose={() => (settingsOpen = false)}
      onSave={saveSettings}
      onTestNotification={testNotification}
    />
  {/if}

  {#if toast}<div class="toast" role="status">{toast}</div>{/if}
</div>
