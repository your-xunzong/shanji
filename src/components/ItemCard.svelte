<script lang="ts">
  import {
    itemTiming,
    localInputToIso,
    nextReminderLabel,
    toLocalDateTimeInput,
  } from '../lib/presentation';
  import type { Category, Item, Tag, UpdateItemInput } from '../lib/types';

  export let item: Item;
  export let busy = false;
  export let onComplete: (item: Item, completed: boolean) => void;
  export let onPause: (item: Item, paused: boolean) => void;
  export let onReschedule: (item: Item, dueAt: string) => Promise<void>;
  export let categories: Category[] = [];
  export let tags: Tag[] = [];
  export let onUpdate: (item: Item, input: UpdateItemInput) => Promise<void>;
  export let onDelete: (item: Item, deleted: boolean) => Promise<void>;
  export let onPermanentDelete: (item: Item) => Promise<void>;

  let editingTime = false;
  let dueDraft = '';
  let editError = '';
  let editingItem = false;
  let titleDraft = '';
  let notesDraft = '';
  let categoryDraft: string | null = null;
  let tagDrafts: string[] = [];
  let mustDraft = false;
  let repeatDraft = 30;

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

  function beginEdit(): void {
    titleDraft = item.title;
    notesDraft = item.notes;
    categoryDraft = item.categoryId;
    tagDrafts = item.tags.map((tag) => tag.id);
    dueDraft = toLocalDateTimeInput(item.dueAt);
    mustDraft = item.completionPolicy === 'MUST_COMPLETE_TODAY';
    repeatDraft = item.repeatIntervalMinutes ?? 30;
    editError = '';
    editingItem = true;
    editingTime = false;
  }

  function toggleTag(id: string): void {
    tagDrafts = tagDrafts.includes(id)
      ? tagDrafts.filter((value) => value !== id)
      : [...tagDrafts, id];
  }

  async function saveItem(): Promise<void> {
    const dueAt = localInputToIso(dueDraft);
    if (!titleDraft.trim()) {
      editError = '请填写事项内容';
      return;
    }
    if (!dueAt) {
      editError = '请选择有效的日期和时间';
      return;
    }
    editError = '';
    try {
      await onUpdate(item, {
        title: titleDraft,
        notes: notesDraft,
        categoryId: categoryDraft,
        tagIds: tagDrafts,
        dueAt,
        mustCompleteToday: mustDraft,
        repeatIntervalMinutes: mustDraft ? repeatDraft : null,
      });
      editingItem = false;
    } catch {
      editError = '修改没有保存，事项仍保持原样';
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

  {#if item.status !== 'DELETED'}
    <button
      class="complete-toggle"
      class:checked={item.status === 'DONE'}
      aria-label={item.status === 'DONE' ? `恢复事项：${item.title}` : `完成事项：${item.title}`}
      disabled={busy}
      on:click={() => onComplete(item, item.status !== 'DONE')}
    >
      <svg viewBox="0 0 20 20" aria-hidden="true"><path d="m5 10 3 3 7-7" /></svg>
    </button>
  {:else}<span class="deleted-marker" aria-hidden="true">×</span>{/if}

  <div class="item-content">
    <div class="item-title-row">
      <h3>{item.title}</h3>
      {#if item.categoryName}
        <span class="category-tag" style={`--category-color:${categories.find((value) => value.id === item.categoryId)?.color ?? '#687789'}`}>{item.categoryName}</span>
      {/if}
    </div>
    {#if item.notes}<p class="item-notes">{item.notes}</p>{/if}
    <div class="item-meta">
      <span class="timing-label" data-tone={timing.tone}>{timing.label}</span>
      {#if item.rolloverCount > 0}<span>已顺延 {item.rolloverCount} 次</span>{/if}
      {#if reminder}<span>{reminder}</span>{/if}
    </div>
    {#if item.tags.length > 0}
      <div class="item-tag-list" aria-label="事项标签">
        {#each item.tags as tag}<span style={`--tag-color:${tag.color}`}>{tag.name}</span>{/each}
      </div>
    {/if}
    {#if editingTime}
      <div class="reschedule-editor">
        <input type="datetime-local" bind:value={dueDraft} aria-label={`调整事项时间：${item.title}`} />
        <button class="primary-mini" disabled={busy || !dueDraft} on:click={saveReschedule}>保存时间</button>
        <button class="text-mini" disabled={busy} on:click={() => (editingTime = false)}>取消</button>
        {#if editError}<span role="alert">{editError}</span>{/if}
      </div>
    {/if}
    {#if editingItem}
      <div class="item-editor">
        <label class="editor-wide"><span>事项内容</span><input type="text" maxlength="4000" bind:value={titleDraft} /></label>
        <label class="editor-wide"><span>备注</span><textarea rows="2" bind:value={notesDraft}></textarea></label>
        <label><span>类型</span><select bind:value={categoryDraft}><option value={null}>收件箱</option>{#each categories as category}<option value={category.id}>{category.name}</option>{/each}</select></label>
        <label><span>提醒时间</span><input type="datetime-local" bind:value={dueDraft} /></label>
        <div class="editor-wide tag-picker"><span>标签</span><div>{#each tags as tag}<button type="button" class:active={tagDrafts.includes(tag.id)} style={`--tag-color:${tag.color}`} on:click={() => toggleTag(tag.id)}>{tag.name}</button>{/each}{#if tags.length === 0}<small>可在“类型与标签”中添加</small>{/if}</div></div>
        <label class="editor-check editor-wide"><input type="checkbox" bind:checked={mustDraft} /><span>今日必做：到期后继续提醒</span></label>
        {#if mustDraft}<label><span>重复间隔</span><select bind:value={repeatDraft}><option value={15}>15 分钟</option><option value={30}>30 分钟</option><option value={60}>60 分钟</option><option value={120}>2 小时</option></select></label>{/if}
        <div class="editor-actions editor-wide"><button class="primary-mini" disabled={busy} on:click={saveItem}>保存修改</button><button class="text-mini" disabled={busy} on:click={() => (editingItem = false)}>取消</button></div>
        {#if editError}<span class="editor-wide" role="alert">{editError}</span>{/if}
      </div>
    {/if}
  </div>

  {#if item.status === 'DELETED'}
    <div class="item-actions">
      <button class="time-action" disabled={busy} on:click={() => onDelete(item, false)}>恢复</button>
      <button class="danger-mini" disabled={busy} on:click={() => onPermanentDelete(item)}>永久删除</button>
    </div>
  {:else}
    <div class="item-actions">
      <button class="time-action" disabled={busy} on:click={beginEdit}>编辑</button>
      {#if item.status === 'OPEN'}<button class="time-action" disabled={busy} on:click={beginReschedule}>调整时间</button>{/if}
      <button class="danger-mini" disabled={busy} on:click={() => onDelete(item, true)}>删除</button>
      {#if item.status === 'OPEN' && item.completionPolicy === 'MUST_COMPLETE_TODAY'}
        <button class="quiet-action" disabled={busy} on:click={() => onPause(item, !item.reminderPaused)}>
          {item.reminderPaused ? '恢复提醒' : '暂停提醒'}
        </button>
      {/if}
    </div>
  {/if}
</article>
