<script lang="ts">
  import {
    itemTiming,
    localInputToIso,
    nextReminderLabel,
    toLocalDateTimeInput,
  } from '../lib/presentation';
  import type { Item } from '../lib/types';

  export let item: Item;
  export let busy = false;
  export let onComplete: (item: Item, completed: boolean) => void;
  export let onPause: (item: Item, paused: boolean) => void;
  export let onReschedule: (item: Item, dueAt: string) => Promise<void>;

  let editingTime = false;
  let dueDraft = '';
  let editError = '';

  $: timing = itemTiming(item);
  $: reminder = nextReminderLabel(item);

  function beginReschedule(): void {
    dueDraft = toLocalDateTimeInput(item.dueAt);
    editError = '';
    editingTime = true;
  }

  async function saveReschedule(): Promise<void> {
    const dueAt = localInputToIso(dueDraft);
    if (!dueAt) {
      editError = '请选择有效的日期和时间';
      return;
    }
    editError = '';
    try {
      await onReschedule(item, dueAt);
      editingTime = false;
    } catch {
      editError = '时间没有保存，请重试';
    }
  }
</script>

<article
  class="item-card"
  class:done={item.status === 'DONE'}
  class:urgent={item.completionPolicy === 'MUST_COMPLETE_TODAY'}
  data-item-id={item.id}
>
  <div class="time-rail" data-tone={timing.tone} aria-hidden="true">
    {#if item.nextReminderAt && !item.reminderPaused}<span class="rail-tick"></span>{/if}
  </div>

  <button
    class="complete-toggle"
    class:checked={item.status === 'DONE'}
    aria-label={item.status === 'DONE' ? `恢复事项：${item.title}` : `完成事项：${item.title}`}
    disabled={busy}
    on:click={() => onComplete(item, item.status !== 'DONE')}
  >
    <svg viewBox="0 0 20 20" aria-hidden="true"><path d="m5 10 3 3 7-7" /></svg>
  </button>

  <div class="item-content">
    <div class="item-title-row">
      <h3>{item.title}</h3>
      {#if item.categoryName}
        <span class="category-tag">{item.categoryName}</span>
      {/if}
    </div>
    {#if item.notes}<p class="item-notes">{item.notes}</p>{/if}
    <div class="item-meta">
      <span class="timing-label" data-tone={timing.tone}>{timing.label}</span>
      {#if item.rolloverCount > 0}<span>已顺延 {item.rolloverCount} 次</span>{/if}
      {#if reminder}<span>{reminder}</span>{/if}
    </div>
    {#if editingTime}
      <div class="reschedule-editor">
        <input type="datetime-local" bind:value={dueDraft} aria-label={`调整事项时间：${item.title}`} />
        <button class="primary-mini" disabled={busy || !dueDraft} on:click={saveReschedule}>保存时间</button>
        <button class="text-mini" disabled={busy} on:click={() => (editingTime = false)}>取消</button>
        {#if editError}<span role="alert">{editError}</span>{/if}
      </div>
    {/if}
  </div>

  {#if item.status === 'OPEN'}
    <div class="item-actions">
      <button class="time-action" disabled={busy} on:click={beginReschedule}>调整时间</button>
      {#if item.completionPolicy === 'MUST_COMPLETE_TODAY'}
        <button class="quiet-action" disabled={busy} on:click={() => onPause(item, !item.reminderPaused)}>
          {item.reminderPaused ? '恢复提醒' : '暂停提醒'}
        </button>
      {/if}
    </div>
  {/if}
</article>
