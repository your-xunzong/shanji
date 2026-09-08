<script lang="ts">
  import { onMount } from 'svelte';
  import { isTauri } from '@tauri-apps/api/core';
  import { api } from '../lib/api';
  import type { DueNotification } from '../lib/types';

  let queue: DueNotification[] = [];
  let busy = false;
  let dismissing = false;
  let error = '';
  let unlisten: (() => void) | undefined;
  let unlistenHidden: (() => void) | undefined;
  $: current = queue[0];

  onMount(() => {
    if (isTauri()) {
      void import('@tauri-apps/api/event').then(async ({ listen }) => {
        unlisten = await listen<DueNotification>('overlay_reminder', ({ payload }) => {
          if (!queue.some((item) => item.eventId === payload.eventId)) queue = [...queue, payload];
        });
        unlistenHidden = await listen('overlay_hidden', () => {
          queue = [];
          error = '';
        });
      });
    } else {
      queue = [{
        eventId: 'preview-overlay',
        itemId: 'preview-item',
        title: '下班前提交发布审批',
        dueLocalDate: '2026-08-29',
        dueLocalTime: '18:00',
        completionPolicy: 'MUST_COMPLETE_TODAY',
        eventKind: 'TODAY_MUST',
        reminderPlan: 'FORCE',
        tagIds: [],
      }];
    }
    return () => {
      unlisten?.();
      unlistenHidden?.();
    };
  });

  async function finish(action: 'complete' | 'snooze' | 'open'): Promise<void> {
    if (!current || busy) return;
    busy = true;
    error = '';
    try {
      if (action === 'complete') await api.setItemCompleted(current.itemId, true);
      if (action === 'snooze') await api.snoozeItem(current.itemId, 15);
      if (action === 'open') await api.showMain();
      queue = queue.slice(1);
      if (queue.length === 0) await api.hideReminder();
    } catch (cause) {
      error = cause instanceof Error && cause.message ? cause.message : '操作没有保存，请打开闪记重试。';
    } finally {
      busy = false;
    }
  }

  async function dismissWindow(): Promise<void> {
    if (dismissing) return;
    dismissing = true;
    error = '';
    try {
      await api.hideReminder();
      queue = [];
    } catch {
      error = '提醒窗没有关闭。你可以重试，或打开闪记处理这条提醒。';
    } finally {
      dismissing = false;
    }
  }

  function handleKeydown(event: KeyboardEvent): void {
    if (event.key === 'Escape' && !event.isComposing) {
      event.preventDefault();
      void dismissWindow();
    }
  }
</script>

<svelte:window on:keydown={handleKeydown} />

<main class="reminder-shell">
  <div class="reminder-topline">
    <div class="reminder-drag-area" data-tauri-drag-region>
      <span class="brand-mark" aria-hidden="true"></span>
      <strong>闪记提醒</strong>
      {#if queue.length > 1}<span>{queue.length} 条待处理</span>{/if}
    </div>
    <button class="icon-button reminder-close" aria-label="关闭本次提醒窗" title="关闭本次提醒窗，不会完成或暂停事项" disabled={dismissing} on:click={dismissWindow}>×</button>
  </div>
  {#if current}
    <div class="reminder-body">
      <p>{current.completionPolicy === 'MUST_COMPLETE_TODAY' ? '今日必做 · 到期后持续提醒' : '事项已到期'}</p>
      <h1>{current.title}</h1>
      <span>到期：{current.dueLocalDate} {current.dueLocalTime}</span>
      {#if error}<small role="alert">{error}</small>{/if}
    </div>
    <div class="reminder-actions">
      <button class="primary-button" disabled={busy} on:click={() => finish('complete')}>完成</button>
      <button class="secondary-button" disabled={busy} on:click={() => finish('snooze')}>15 分钟后提醒</button>
      <button class="text-mini" disabled={busy} on:click={() => finish('open')}>打开闪记</button>
    </div>
  {:else}
    <div class="reminder-waiting" data-tauri-drag-region>
      <span>{error || '提醒内容正在载入；如果一直没有出现，可以关闭本次提醒窗。'}</span>
    </div>
  {/if}
</main>
