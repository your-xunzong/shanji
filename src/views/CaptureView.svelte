<script lang="ts">
  import { onMount } from 'svelte';
  import { isTauri } from '@tauri-apps/api/core';
  import { api } from '../lib/api';
  import {
    EVENT_KIND_OPTIONS,
    duePreview,
    localDateTimeInputLabel,
    localInputToIso,
    mustCompletePreview,
  } from '../lib/presentation';
  import type { EventKind, ScheduleUnit, Settings, Tag } from '../lib/types';

  let content = '';
  let eventKind: EventKind | '' = '';
  let mustCompleteToday = false;
  let important = false;
  let dueAt = '';
  let timePickerOpen = false;
  let settings: Settings | null = null;
  let tags: Tag[] = [];
  let tagIds: string[] = [];
  let tagPickerOpen = false;
  let saving = false;
  let saved = false;
  let error = '';
  let inputElement: HTMLTextAreaElement;
  let draftTimer: ReturnType<typeof setTimeout> | undefined;
  let unlisten: (() => void) | undefined;
  let eventPickerOpen = false;
  let startAt = '';
  let endAt = '';
  let targetAt = '';
  let leadValue = 1;
  let leadUnit: ScheduleUnit = 'DAY';
  let cadenceValue = 1;
  let cadenceUnit: ScheduleUnit = 'DAY';

  $: mustCompleteToday = eventKind === 'TODAY_MUST';
  $: dueLabel = dueAt
    ? `指定：${localDateTimeInputLabel(dueAt)}`
    : mustCompleteToday
      ? mustCompletePreview(settings?.defaultDueTime ?? '18:00')
      : duePreview(settings?.defaultDueTime ?? '18:00', settings?.workdays ?? [1, 2, 3, 4, 5]);

  onMount(() => {
    void refreshContext();
    if (isTauri()) {
      void import('@tauri-apps/api/event').then(async ({ listen }) => {
        unlisten = await listen('capture_opened', () => void refreshContext());
      });
    }

    return () => {
      if (draftTimer) clearTimeout(draftTimer);
      unlisten?.();
    };
  });

  async function refreshContext(): Promise<void> {
    try {
      const [loadedSettings, loadedTags, draft] = await Promise.all([
        api.getSettings(),
        api.listTags(),
        api.loadDraft(),
      ]);
      settings = loadedSettings;
      tags = loadedTags;
      if (!content) content = draft;
    } catch (cause) {
      error = readableError(cause, '无法读取本地设置，仍可输入内容。');
    } finally {
      requestAnimationFrame(() => inputElement?.focus());
    }
  }

  function readableError(cause: unknown, fallback: string): string {
    return cause instanceof Error && cause.message ? cause.message : fallback;
  }

  function persistDraft(): void {
    saved = false;
    error = '';
    if (draftTimer) clearTimeout(draftTimer);
    draftTimer = setTimeout(() => {
      void api.saveDraft(content).catch(() => {
        error = '草稿暂时无法保存。请复制文字后检查数据目录。';
      });
    }, 250);
  }

  function localDateTimeValue(date: Date, time: string): string {
    const [hours, minutes] = time.split(':').map(Number);
    const value = new Date(date);
    value.setHours(hours, minutes, 0, 0);
    const offset = value.getTimezoneOffset();
    return new Date(value.getTime() - offset * 60_000).toISOString().slice(0, 16);
  }

  function nextWorkdayDate(from: Date): Date {
    const workdays = settings?.workdays ?? [1, 2, 3, 4, 5];
    const result = new Date(from);
    for (let offset = 1; offset <= 14; offset += 1) {
      result.setDate(result.getDate() + 1);
      const day = result.getDay() === 0 ? 7 : result.getDay();
      if (workdays.includes(day)) return result;
    }
    return result;
  }

  function chooseDefault(): void {
    dueAt = '';
    timePickerOpen = false;
  }

  function chooseToday(): void {
    dueAt = localDateTimeValue(new Date(), settings?.defaultDueTime ?? '18:00');
    timePickerOpen = false;
  }

  function chooseNextWorkday(): void {
    dueAt = localDateTimeValue(nextWorkdayDate(new Date()), settings?.defaultDueTime ?? '18:00');
    timePickerOpen = false;
  }

  async function submit(): Promise<void> {
    const title = content.trim();
    if (!title || saving) return;

    saving = true;
    error = '';
    try {
      if (eventKind === 'WARNING' && !targetAt) {
        throw new Error('请设置预警事件的发生时间，或先选择“稍后选择事件类型”。');
      }
      if (eventKind === 'CONTINUOUS' && (!startAt || !endAt)) {
        throw new Error('请设置持续事件的开始和结束时间，或先选择“稍后选择事件类型”。');
      }
      await api.createItem({
        title,
        categoryId: null,
        dueAt: localInputToIso(dueAt),
        mustCompleteToday,
        repeatIntervalMinutes: mustCompleteToday
          ? settings?.overtimeIntervalMinutes ?? 30
          : null,
        tagIds,
        event: {
          kind: eventKind || null,
          reminderPlan: null,
          important,
          startAt: localInputToIso(startAt),
          endAt: localInputToIso(endAt),
          targetAt: localInputToIso(targetAt),
          leadValue: eventKind === 'WARNING' ? leadValue : null,
          leadUnit: eventKind === 'WARNING' ? leadUnit : null,
          cadenceValue: eventKind === 'CONTINUOUS' ? cadenceValue : null,
          cadenceUnit: eventKind === 'CONTINUOUS' ? cadenceUnit : null,
          emphasisMaxPerDay: null,
        },
      });
      content = '';
      eventKind = '';
      important = false;
      dueAt = '';
      startAt = '';
      endAt = '';
      targetAt = '';
      tagIds = [];
      saved = true;
      await new Promise((resolve) => setTimeout(resolve, 160));
      await api.hideCapture();
      saved = false;
    } catch (cause) {
      error = readableError(cause, '保存失败，内容仍在输入框中。请重试。');
      inputElement?.focus();
    } finally {
      saving = false;
    }
  }

  function handleKeydown(event: KeyboardEvent): void {
    if (event.key === 'Escape') {
      event.preventDefault();
      void api.hideCapture();
      return;
    }

    if (
      event.key === 'Enter' &&
      !event.shiftKey &&
      !event.isComposing &&
      !event.ctrlKey &&
      !event.metaKey
    ) {
      event.preventDefault();
      void submit();
    }
  }

  function toggleTag(id: string): void {
    tagIds = tagIds.includes(id) ? tagIds.filter((value) => value !== id) : [...tagIds, id];
  }

  function toggleTimePicker(): void {
    timePickerOpen = !timePickerOpen;
    if (timePickerOpen) tagPickerOpen = false;
  }

  function toggleTagPicker(): void {
    tagPickerOpen = !tagPickerOpen;
    if (tagPickerOpen) timePickerOpen = false;
  }

  function handleEventKindChange(): void {
    eventPickerOpen = eventKind === 'WARNING' || eventKind === 'CONTINUOUS';
    if (eventPickerOpen) {
      timePickerOpen = false;
      tagPickerOpen = false;
    }
  }
</script>

<svelte:head>
  <title>快速记录 · 闪记</title>
</svelte:head>

<main class="capture-shell" class:is-saved={saved} data-tauri-drag-region>
  <div class="capture-topline" data-tauri-drag-region>
    <span class="brand-mark" aria-hidden="true"></span>
    <span class="capture-brand">闪记</span>
    <span class="capture-status" aria-live="polite">
      {saved ? '已可靠保存' : saving ? '正在保存…' : '本地保存'}
    </span>
    <button class="icon-button capture-close" aria-label="关闭快速录入" on:click={() => api.hideCapture()}>
      <svg viewBox="0 0 20 20" aria-hidden="true"><path d="m5 5 10 10M15 5 5 15" /></svg>
    </button>
  </div>

  <label class="visually-hidden" for="capture-input">备忘内容</label>
  <textarea
    id="capture-input"
    bind:this={inputElement}
    bind:value={content}
    class="capture-input"
    rows="3"
    maxlength="4000"
    placeholder="写下要记的事…"
    on:input={persistDraft}
    on:keydown={handleKeydown}
  ></textarea>

  <div class="capture-controls">
    <button
      type="button"
      class="chip field-chip"
      class:active={Boolean(dueAt)}
      aria-expanded={timePickerOpen}
      aria-controls="capture-time-picker"
      on:click={toggleTimePicker}
    >
      <svg viewBox="0 0 20 20" aria-hidden="true"><circle cx="10" cy="10" r="7" /><path d="M10 6v4l3 2" /></svg>
      <span>{dueLabel}</span>
      <span class="chip-chevron" aria-hidden="true">⌄</span>
    </button>

    {#if timePickerOpen}
      <div class="capture-time-picker" id="capture-time-picker">
        <div class="time-picker-heading">
          <div><strong>什么时候提醒</strong><small>选择后会显示最终时间</small></div>
          <button class="icon-button" aria-label="关闭时间选择" on:click={() => (timePickerOpen = false)}>×</button>
        </div>
        <div class="time-preset-grid">
          <button class:active={!dueAt} on:click={chooseDefault}>使用默认规则</button>
          <button on:click={chooseToday}>今天 {settings?.defaultDueTime ?? '18:00'}</button>
          <button on:click={chooseNextWorkday}>下一工作日</button>
        </div>
        <label class="custom-time-field">
          <span>指定日期和时间</span>
          <input type="datetime-local" bind:value={dueAt} on:change={() => (timePickerOpen = false)} />
        </label>
      </div>
    {/if}

    <label class="chip select-chip" class:active={Boolean(eventKind)}>
      <svg viewBox="0 0 20 20" aria-hidden="true"><path d="M3.5 5.5h5l1.5 2h6.5v8h-13z" /></svg>
      <select bind:value={eventKind} aria-label="选择事件类型" on:change={handleEventKindChange}>
        <option value="">稍后选择事件类型</option>
        {#each EVENT_KIND_OPTIONS as option}
          <option value={option.value}>{option.label}</option>
        {/each}
      </select>
    </label>

    {#if eventPickerOpen && eventKind === 'WARNING'}
      <div class="capture-event-picker">
        <div class="time-picker-heading"><div><strong>设置预警</strong><small>从提前时间开始，每天提醒</small></div><button class="icon-button" aria-label="关闭预警设置" on:click={() => (eventPickerOpen = false)}>×</button></div>
        <label class="custom-time-field"><span>事件发生时间</span><input type="datetime-local" bind:value={targetAt} /></label>
        <div class="event-rule-row"><label><span>提前</span><input type="number" min="1" max="999" bind:value={leadValue} /></label><label><span>单位</span><select bind:value={leadUnit}><option value="DAY">天</option><option value="WEEK">周</option><option value="MONTH">月</option><option value="YEAR">年</option></select></label></div>
        <button class="primary-mini" disabled={!targetAt} on:click={() => (eventPickerOpen = false)}>设置好了</button>
      </div>
    {:else if eventPickerOpen && eventKind === 'CONTINUOUS'}
      <div class="capture-event-picker">
        <div class="time-picker-heading"><div><strong>设置持续时间</strong><small>只在这段时间内按周期提醒</small></div><button class="icon-button" aria-label="关闭持续事件设置" on:click={() => (eventPickerOpen = false)}>×</button></div>
        <label class="custom-time-field"><span>开始</span><input type="datetime-local" bind:value={startAt} /></label>
        <label class="custom-time-field"><span>结束</span><input type="datetime-local" bind:value={endAt} /></label>
        <div class="event-rule-row"><label><span>每</span><input type="number" min="1" max="999" bind:value={cadenceValue} /></label><label><span>周期</span><select bind:value={cadenceUnit}><option value="DAY">天</option><option value="WEEK">周</option><option value="MONTH">月</option><option value="YEAR">年</option></select></label></div>
        <button class="primary-mini" disabled={!startAt || !endAt} on:click={() => (eventPickerOpen = false)}>设置好了</button>
      </div>
    {/if}

    {#if tags.length > 0}
      <button type="button" class="chip capture-tag-button" class:active={tagIds.length > 0} aria-expanded={tagPickerOpen} on:click={toggleTagPicker}>
        <span aria-hidden="true">#</span>{tagIds.length > 0 ? `${tagIds.length} 个标签` : '标签'}
      </button>
      {#if tagPickerOpen}
        <div class="capture-tag-picker">
          <strong>选择标签</strong>
          {#each tags as tag}
            <label style={`--tag-color:${tag.color}`}><input type="checkbox" checked={tagIds.includes(tag.id)} on:change={() => toggleTag(tag.id)} /><span></span>{tag.name}</label>
          {/each}
          <button class="primary-mini" on:click={() => (tagPickerOpen = false)}>选好了</button>
        </div>
      {/if}
    {/if}

    <label class="must-chip" class:active={eventKind === 'TODAY_MUST'}>
      <input type="checkbox" checked={eventKind === 'TODAY_MUST'} on:change={(event) => (eventKind = event.currentTarget.checked ? 'TODAY_MUST' : '')} />
      <span class="must-dot" aria-hidden="true"></span>
      今日必做
      {#if mustCompleteToday}
        <span class="interval">每 {settings?.overtimeIntervalMinutes ?? 30} 分钟</span>
      {/if}
    </label>

    <div class="capture-spacer"></div>
    <span class="key-help"><kbd>Enter</kbd> 保存</span>
    <button class="save-button" disabled={!content.trim() || saving} on:click={submit}>
      {saving ? '保存中' : '记下'}
    </button>
  </div>

  {#if error}
    <div class="capture-error" role="alert">
      <span>{error}</span>
      <button on:click={() => navigator.clipboard.writeText(content)}>复制内容</button>
    </div>
  {/if}
</main>
