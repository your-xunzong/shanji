<script lang="ts">
  import {
    EVENT_KIND_OPTIONS,
    REMINDER_PLAN_OPTIONS,
    eventKindLabel,
    itemTiming,
    localInputToIso,
    nextReminderLabel,
    toLocalDateTimeInput,
  } from '../lib/presentation';
  import type { Category, EventKind, Item, ReminderPlan, RepeatTimeMode, ScheduleUnit, Tag, UpdateItemInput } from '../lib/types';

  export let item: Item;
  export let busy = false;
  export let onComplete: (item: Item, completed: boolean) => void;
  export let onPause: (item: Item, paused: boolean) => void;
  export let onReschedule: (item: Item, dueAt: string) => Promise<void>;
  export let categories: Category[] = [];
  export let tags: Tag[] = [];
  export let repeatDefaultTimes: string[] = ['10:00', '17:00'];
  export let onUpdate: (item: Item, input: UpdateItemInput) => Promise<void>;
  export let onDelete: (item: Item, deleted: boolean) => Promise<void>;
  export let onPermanentDelete: (item: Item) => Promise<void>;
  export let onCompleteOccurrence: (item: Item) => Promise<void> = async () => {};
  export let openEditor = false;

  let editingTime = false;
  let dueDraft = '';
  let editError = '';
  let editingItem = false;
  let titleDraft = '';
  let notesDraft = '';
  let categoryDraft: string | null = null;
  let tagDrafts: string[] = [];
  let repeatDraft = 30;
  let eventKindDraft: EventKind | '' = '';
  let reminderPlanDraft: ReminderPlan = 'ONCE';
  let importantDraft = false;
  let startDraft = '';
  let endDraft = '';
  let targetDraft = '';
  let leadValueDraft = 1;
  let leadUnitDraft: ScheduleUnit = 'DAY';
  let cadenceValueDraft = 1;
  let cadenceUnitDraft: ScheduleUnit = 'DAY';
  let emphasisMaxDraft = 8;
  let repeatTimeModeDraft: RepeatTimeMode = 'SPECIFIED';
  let repeatSpecifiedTimeDraft = '10:00';
  let repeatTimesDraft: string[] = ['10:00', '17:00'];
  let openedFromParent = false;

  $: timing = itemTiming(item);
  $: reminder = nextReminderLabel(item);
  $: if (openEditor && !openedFromParent) {
    openedFromParent = true;
    beginEdit();
  }
  $: if (!openEditor) openedFromParent = false;

  function matchesSeries(value: Item): boolean {
    return value.eventKind === 'MONTHLY' || value.eventKind === 'YEARLY';
  }

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
    repeatDraft = item.repeatIntervalMinutes ?? 30;
    eventKindDraft = item.eventKind ?? '';
    reminderPlanDraft = item.reminderPlan;
    importantDraft = item.important;
    startDraft = item.startAt ? toLocalDateTimeInput(item.startAt) : '';
    endDraft = item.endAt ? toLocalDateTimeInput(item.endAt) : '';
    targetDraft = item.targetAt ? toLocalDateTimeInput(item.targetAt) : '';
    leadValueDraft = item.leadValue ?? 1;
    leadUnitDraft = item.leadUnit ?? 'DAY';
    cadenceValueDraft = item.cadenceValue ?? 1;
    cadenceUnitDraft = item.cadenceUnit ?? 'DAY';
    emphasisMaxDraft = item.emphasisMaxPerDay;
    repeatTimeModeDraft = item.repeatTimeMode;
    repeatSpecifiedTimeDraft = item.repeatTimes[0] ?? item.dueLocalTime;
    repeatTimesDraft = item.repeatTimes.length === 2 ? [...item.repeatTimes] : [...repeatDefaultTimes];
    editError = '';
    editingItem = true;
    editingTime = false;
  }

  function toggleTag(id: string): void {
    tagDrafts = tagDrafts.includes(id)
      ? tagDrafts.filter((value) => value !== id)
      : [...tagDrafts, id];
  }

  function useDefaultRepeatTimes(): void {
    if (repeatTimeModeDraft !== 'DEFAULT') repeatTimesDraft = [...repeatDefaultTimes];
    repeatTimeModeDraft = 'DEFAULT';
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
    if (eventKindDraft === 'WARNING' && !targetDraft) {
      editError = '请设置预警事件的发生时间';
      return;
    }
    if (eventKindDraft === 'CONTINUOUS' && (!startDraft || !endDraft)) {
      editError = '请设置持续事件的开始和结束时间';
      return;
    }
    if (reminderPlanDraft === 'REPEAT' && repeatTimeModeDraft === 'SPECIFIED' && !repeatSpecifiedTimeDraft) {
      editError = '请选择每天提醒的时间';
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
        mustCompleteToday: eventKindDraft === 'TODAY_MUST',
        repeatIntervalMinutes: reminderPlanDraft === 'EMPHASIS' || reminderPlanDraft === 'FORCE' ? repeatDraft : null,
        event: {
          kind: eventKindDraft || null,
          reminderPlan: reminderPlanDraft,
          important: importantDraft,
          startAt: localInputToIso(startDraft),
          endAt: localInputToIso(endDraft),
          targetAt: localInputToIso(targetDraft),
          leadValue: eventKindDraft === 'WARNING' ? leadValueDraft : null,
          leadUnit: eventKindDraft === 'WARNING' ? leadUnitDraft : null,
          cadenceValue: eventKindDraft === 'CONTINUOUS' || reminderPlanDraft === 'CUSTOM' ? cadenceValueDraft : null,
          cadenceUnit: eventKindDraft === 'CONTINUOUS' || reminderPlanDraft === 'CUSTOM' ? cadenceUnitDraft : null,
          emphasisMaxPerDay: reminderPlanDraft === 'EMPHASIS' ? emphasisMaxDraft : null,
          repeatTimeMode: reminderPlanDraft === 'REPEAT' ? repeatTimeModeDraft : null,
          repeatTimes: reminderPlanDraft === 'REPEAT'
            ? repeatTimeModeDraft === 'DEFAULT'
              ? repeatTimesDraft
              : [repeatSpecifiedTimeDraft]
            : [],
        },
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
      aria-label={item.status === 'DONE' ? `恢复事项：${item.title}` : matchesSeries(item) ? `结束整个系列：${item.title}` : `完成事项：${item.title}`}
      disabled={busy}
      on:click={() => onComplete(item, item.status !== 'DONE')}
    >
      <svg viewBox="0 0 20 20" aria-hidden="true"><path d="m5 10 3 3 7-7" /></svg>
    </button>
  {:else}<span class="deleted-marker" aria-hidden="true">×</span>{/if}

  <div class="item-content">
    <div class="item-title-row">
      <h3>{item.title}</h3>
      <span class="category-tag event-kind-tag" class:pending={!item.eventKind}>{eventKindLabel(item.eventKind)}</span>
      {#if item.categoryName}<span class="category-tag" style={`--category-color:${categories.find((value) => value.id === item.categoryId)?.color ?? '#687789'}`}>{item.categoryName}</span>{/if}
      {#if item.important}<span class="important-flag">重要</span>{/if}
    </div>
    {#if item.notes}<p class="item-notes">{item.notes}</p>{/if}
    <div class="item-meta">
      <span class="timing-label" data-tone={timing.tone}>{timing.label}</span>
      {#if item.rolloverCount > 0}<span>已顺延 {item.rolloverCount} 次</span>{/if}
      {#if reminder}<span>{reminder}</span>{/if}
      <span>{REMINDER_PLAN_OPTIONS.find((entry) => entry.value === item.reminderPlan)?.label ?? item.reminderPlan}提醒</span>
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
        <label><span>事件类型</span><select bind:value={eventKindDraft}><option value="">稍后选择事件类型</option>{#each EVENT_KIND_OPTIONS as option}<option value={option.value}>{option.label}</option>{/each}</select></label>
        <label><span>类型</span><select bind:value={categoryDraft}><option value={null}>未分类</option>{#each categories as category}<option value={category.id}>{category.name}</option>{/each}</select></label>
        <label><span>提醒方案</span><select bind:value={reminderPlanDraft}>{#each REMINDER_PLAN_OPTIONS as option}<option value={option.value}>{option.label}</option>{/each}</select></label>
        <label><span>提醒时间</span><input type="datetime-local" bind:value={dueDraft} /></label>
        <div class="editor-wide tag-picker"><span>标签</span><div>{#each tags as tag}<button type="button" class:active={tagDrafts.includes(tag.id)} style={`--tag-color:${tag.color}`} on:click={() => toggleTag(tag.id)}>{tag.name}</button>{/each}{#if tags.length === 0}<small>可在“整理方式”中添加</small>{/if}</div></div>
        <label class="editor-check editor-wide"><input type="checkbox" bind:checked={importantDraft} /><span>标记为重要</span></label>
        {#if reminderPlanDraft === 'EMPHASIS' || reminderPlanDraft === 'FORCE'}<label><span>再次提醒间隔</span><select bind:value={repeatDraft}><option value={15}>15 分钟</option><option value={30}>30 分钟</option><option value={60}>60 分钟</option><option value={120}>2 小时</option></select></label>{/if}
        {#if reminderPlanDraft === 'REPEAT'}
          <div class="editor-wide repeat-plan-editor">
            <span>每天提醒时间</span>
            <div class="repeat-plan-options">
              <label><input type="radio" checked={repeatTimeModeDraft === 'DEFAULT'} value="DEFAULT" on:change={useDefaultRepeatTimes} /><span>每天两个时间（{repeatTimesDraft.join('、')}）</span></label>
              <label><input type="radio" checked={repeatTimeModeDraft === 'SPECIFIED'} value="SPECIFIED" on:change={() => (repeatTimeModeDraft = 'SPECIFIED')} /><span>每天一个指定时间</span></label>
            </div>
            {#if repeatTimeModeDraft === 'DEFAULT' && repeatTimesDraft.join(',') !== repeatDefaultTimes.join(',')}
              <button class="text-mini repeat-default-action" type="button" on:click={() => (repeatTimesDraft = [...repeatDefaultTimes])}>恢复当前默认（{repeatDefaultTimes.join('、')}）</button>
            {/if}
            {#if repeatTimeModeDraft === 'SPECIFIED'}<input aria-label="每天提醒时间" type="time" bind:value={repeatSpecifiedTimeDraft} />{/if}
          </div>
        {/if}
        {#if reminderPlanDraft === 'EMPHASIS'}<label><span>每天最多提醒</span><input type="number" min="1" max="96" bind:value={emphasisMaxDraft} /></label>{/if}
        {#if eventKindDraft === 'WARNING'}
          <label><span>发生时间</span><input type="datetime-local" bind:value={targetDraft} /></label>
          <label><span>提前数量</span><input type="number" min="1" max="999" bind:value={leadValueDraft} /></label>
          <label><span>提前单位</span><select bind:value={leadUnitDraft}><option value="DAY">天</option><option value="WEEK">周</option><option value="MONTH">月</option><option value="YEAR">年</option></select></label>
        {:else if eventKindDraft === 'CONTINUOUS'}
          <label><span>开始时间</span><input type="datetime-local" bind:value={startDraft} /></label>
          <label><span>结束时间</span><input type="datetime-local" bind:value={endDraft} /></label>
          <label><span>每隔</span><input type="number" min="1" max="999" bind:value={cadenceValueDraft} /></label>
          <label><span>周期单位</span><select bind:value={cadenceUnitDraft}><option value="DAY">天</option><option value="WEEK">周</option><option value="MONTH">月</option><option value="YEAR">年</option></select></label>
        {:else if reminderPlanDraft === 'CUSTOM'}
          <label><span>每隔</span><input type="number" min="1" max="999" bind:value={cadenceValueDraft} /></label>
          <label><span>周期单位</span><select bind:value={cadenceUnitDraft}><option value="DAY">天</option><option value="WEEK">周</option><option value="MONTH">月</option><option value="YEAR">年</option></select></label>
        {/if}
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
      {#if item.status === 'OPEN' && new Date(item.dueAt).getTime() >= Date.now()}<button class="time-action" disabled={busy} on:click={beginReschedule}>调整时间</button>{/if}
      {#if item.status === 'OPEN' && matchesSeries(item)}<button class="time-action" disabled={busy} on:click={() => onCompleteOccurrence(item)}>完成本次</button>{/if}
      <button class="danger-mini" disabled={busy} on:click={() => onDelete(item, true)}>删除</button>
      {#if item.status === 'OPEN'}
        <button class="quiet-action" disabled={busy} on:click={() => onPause(item, !item.reminderPaused)}>
          {item.reminderPaused ? '恢复提醒' : '暂停提醒'}
        </button>
      {/if}
    </div>
  {/if}
</article>
