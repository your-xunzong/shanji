<script lang="ts">
  import { onMount, tick } from 'svelte';
  import { api } from '../lib/api';
  import { EVENT_KIND_OPTIONS, eventKindLabel, localDateKey } from '../lib/presentation';
  import type { Category, EventKind, Tag, TimelineData, TimelineEntry, TimelineMarker, TimelinePosition, TimelineQuery, TimelineScale } from '../lib/types';
  import '../timeline.css';

  export let categories: Category[] = [];
  export let tags: Tag[] = [];
  export let onOpenItem: (id: string) => Promise<void> = async () => {};
  export let onBackToItems: () => void = () => {};

  let scale: TimelineScale = 'MONTH';
  let anchorDate = localDateKey(new Date());
  let includeDone = false;
  let eventKind = '';
  let categoryId = '';
  let tagId = '';
  let data: TimelineData | null = null;
  let loading = true;
  let error = '';
  let actionError = '';
  let filtersOpen = false;
  let requestId = 0;
  let viewport: HTMLDivElement;
  let scrollTop = 0;
  let viewportHeight = 480;
  let activeRow = 0;

  const scaleOptions: { value: TimelineScale; label: string }[] = [
    { value: 'DAY', label: '日' }, { value: 'WEEK', label: '周' }, { value: 'MONTH', label: '月' }, { value: 'YEAR', label: '年' },
  ];
  $: rows = groupEntries(data?.entries ?? []);
  $: firstRow = Math.max(0, Math.floor((scrollTop - 44) / 44) - 5);
  $: lastRow = Math.min(rows.length, firstRow + Math.ceil(viewportHeight / 44) + 12);
  $: visibleRows = rows.slice(firstRow, lastRow);
  $: focusRow = Math.min(activeRow, Math.max(0, rows.length - 1));
  $: rangeLabel = data ? formatRange(data) : '事项甘特图';
  $: chartWidth = data?.scale === 'DAY' ? data.ticks.length * (data.ticks.some((tick) => tick.label.length > 5) ? 96 : 56) : data?.scale === 'WEEK' ? 728 : data?.scale === 'YEAR' ? 960 : (data?.ticks.length ?? 30) * 34;

  onMount(() => {
    void loadTimeline();
    const refresh = () => { if (!document.hidden) void loadTimeline(false); };
    let midnightTimer: ReturnType<typeof setTimeout>;
    function scheduleMidnight() {
      const midnight = new Date();
      midnight.setHours(24, 0, 1, 0);
      midnightTimer = setTimeout(() => { refresh(); scheduleMidnight(); }, midnight.getTime() - Date.now());
    }
    scheduleMidnight();
    window.addEventListener('focus', refresh);
    document.addEventListener('visibilitychange', refresh);
    return () => {
      requestId += 1;
      clearTimeout(midnightTimer);
      window.removeEventListener('focus', refresh);
      document.removeEventListener('visibilitychange', refresh);
    };
  });

  function buildQuery(): TimelineQuery {
    return { scale, anchorDate, includeDone, eventKinds: eventKind ? [eventKind as EventKind] : [], categoryId: categoryId || null, tagIds: tagId ? [tagId] : [] };
  }

  async function loadTimeline(resetScroll = true): Promise<void> {
    const currentRequest = ++requestId;
    const query = buildQuery();
    loading = true;
    error = '';
    try {
      const result = await api.getTimeline(query);
      if (currentRequest !== requestId) return;
      data = result;
      if (resetScroll) {
        activeRow = 0;
        scrollTop = 0;
        if (viewport) viewport.scrollTop = 0;
      }
      // A preference write must never discard successfully loaded items.
      try { await api.saveTimelineView(query.scale, query.includeDone); } catch { /* Keep the usable view. */ }
    } catch {
      if (currentRequest === requestId) error = data
        ? '更新未成功，仍显示上次读取的区间。事项未被修改，可以重试或返回列表。'
        : '甘特图暂时无法读取。事项未被修改，可以重试或返回列表。';
    } finally {
      if (currentRequest === requestId) loading = false;
    }
  }
  async function setScale(value: TimelineScale) { scale = value; await loadTimeline(); }
  async function move(direction: number) {
    const date = parseDate(anchorDate);
    // Start on day 1 so January 31 does not jump over February.
    if (scale === 'MONTH') anchorDate = localDateKey(new Date(date.getFullYear(), date.getMonth() + direction, 1, 12));
    else if (scale === 'YEAR') anchorDate = localDateKey(new Date(date.getFullYear() + direction, 0, 1, 12));
    else { date.setDate(date.getDate() + direction * (scale === 'WEEK' ? 7 : 1)); anchorDate = localDateKey(date); }
    await loadTimeline();
  }
  async function goToday() { anchorDate = localDateKey(new Date()); await loadTimeline(); }
  async function resetFilters() { eventKind = ''; categoryId = ''; tagId = ''; includeDone = false; await loadTimeline(); }
  function parseDate(value: string) { const [y, m, d] = value.split('-').map(Number); return new Date(y, m - 1, d, 12); }
  function formatRange(value: TimelineData) {
    const start = parseDate(value.rangeStart);
    if (value.scale === 'YEAR') return `${start.getFullYear()} 年`;
    if (value.scale === 'MONTH') return `${start.getFullYear()} 年 ${start.getMonth() + 1} 月`;
    const formatter = new Intl.DateTimeFormat('zh-CN', { month: 'long', day: 'numeric' });
    return value.scale === 'DAY' ? `${start.getFullYear()} 年 ${formatter.format(start)}` : `${formatter.format(start)} — ${formatter.format(parseDate(value.rangeEnd))}`;
  }
  function groupEntries(entries: TimelineEntry[]) {
    const grouped = new Map<string, { item: TimelineEntry; segments: TimelineEntry[] }>();
    for (const entry of entries) {
      const row = grouped.get(entry.itemId);
      if (row) row.segments.push(entry);
      else grouped.set(entry.itemId, { item: entry, segments: [entry] });
    }
    return [...grouped.values()];
  }
  function position(value: TimelinePosition) { return `left:${value.left}%;width:${value.width}%`; }
  function relativePosition(value: TimelinePosition, whole: TimelinePosition) {
    if (whole.width <= 0) return 'left:0%;width:100%';
    const left = Math.max(0, (value.left - whole.left) / whole.width * 100);
    const width = Math.min(100 - left, value.width / whole.width * 100);
    return `left:${left}%;width:${Math.max(0, width)}%`;
  }
  function relativeMarker(value: TimelineMarker, whole: TimelinePosition) {
    if (whole.width <= 0) return 'left:100%';
    return `left:${Math.max(0, Math.min(100, (value.position - whole.left) / whole.width * 100))}%`;
  }
  function entryLabel(entry: TimelineEntry) {
    const phases = entry.stateSegments.map((segment) => segment.label).join('；');
    return `${entry.title}：${entry.position.startLabel} 至 ${entry.openEnded ? '今天' : entry.position.endLabel}${entry.occurrenceLabel ? '，' + entry.occurrenceLabel : ''}${entry.status === 'DONE' ? '，已完成' : entry.overdue ? '，已逾期' : ''}${entry.historyIncomplete ? '，历史未记录' : ''}${phases ? `。阶段：${phases}` : ''}`;
  }
  async function openItem(id: string) {
    actionError = '';
    try { await onOpenItem(id); } catch { actionError = '事项暂时无法打开，内容未被修改。请返回列表后重试。'; }
  }
  async function capture() {
    actionError = '';
    try { await api.showCapture(); } catch { actionError = '快速记录窗口未能打开，已有事项未受影响。请返回列表后重试。'; }
  }
  async function navigateRows(event: KeyboardEvent, index: number) {
    if (!event.isComposing && ['ArrowLeft', 'ArrowRight'].includes(event.key) && viewport) {
      event.preventDefault();
      viewport.scrollLeft += event.key === 'ArrowRight' ? 120 : -120;
      return;
    }
    if (event.isComposing || !['ArrowUp', 'ArrowDown', 'Home', 'End'].includes(event.key)) return;
    event.preventDefault();
    activeRow = event.key === 'Home' ? 0 : event.key === 'End' ? rows.length - 1 : Math.max(0, Math.min(rows.length - 1, index + (event.key === 'ArrowDown' ? 1 : -1)));
    if (viewport) {
      const rowTop = activeRow * 44;
      if (rowTop < viewport.scrollTop) viewport.scrollTop = rowTop;
      else if (rowTop + 88 > viewport.scrollTop + viewportHeight) viewport.scrollTop = rowTop + 88 - viewportHeight;
      scrollTop = viewport.scrollTop;
    }
    await tick();
    viewport?.querySelector<HTMLButtonElement>(`button[data-row-index="${activeRow}"]`)?.focus({ preventScroll: true });
  }
</script>

<section class="gantt-view" aria-labelledby="gantt-title">
  <header class="gantt-header">
    <div><p class="gantt-kicker">事项甘特图</p><h1 id="gantt-title">{rangeLabel}</h1></div>
    <button class="new-item-button" on:click={capture}><span aria-hidden="true">＋</span> 快速记录</button>
  </header>
  <div class="gantt-controls" aria-label="甘特图控制">
    <div class="gantt-scales" aria-label="时间范围">{#each scaleOptions as option}<button class:active={scale === option.value} aria-pressed={scale === option.value} on:click={() => setScale(option.value)}>{option.label}</button>{/each}</div>
    <div class="gantt-navigation"><button aria-label="上一段时间" on:click={() => move(-1)}>←</button><button on:click={goToday}>今天</button><button aria-label="下一段时间" on:click={() => move(1)}>→</button></div>
    <span class="gantt-count" role="status">{loading ? '正在读取…' : data ? `${rows.length} 条事项` : ''}</span>
    <button class="gantt-filter-toggle" aria-expanded={filtersOpen} on:click={() => (filtersOpen = !filtersOpen)}>筛选{eventKind || categoryId || tagId || includeDone ? ' · 已选择' : ''}</button>
  </div>
  {#if filtersOpen}
    <form class="gantt-filters" on:submit|preventDefault={() => loadTimeline()}>
      <label><span>事件类型</span><select bind:value={eventKind}><option value="">全部类型</option>{#each EVENT_KIND_OPTIONS as option}<option value={option.value}>{option.label}</option>{/each}</select></label>
      <label><span>分类</span><select bind:value={categoryId}><option value="">全部分类</option>{#each categories as category}<option value={category.id}>{category.name}</option>{/each}</select></label>
      <label><span>标签</span><select bind:value={tagId}><option value="">全部标签</option>{#each tags as tag}<option value={tag.id}>{tag.name}</option>{/each}</select></label>
      <label class="gantt-checkbox"><input type="checkbox" bind:checked={includeDone} />显示已完成</label>
      <button class="primary-button">查看结果</button><button type="button" class="secondary-button" on:click={resetFilters}>清除筛选</button>
    </form>
  {/if}
  {#if error}<div class="gantt-error" role="alert"><span>{error}</span><button on:click={() => loadTimeline()}>重试</button><button on:click={onBackToItems}>返回列表</button></div>{/if}
  {#if actionError}<div class="gantt-error" role="alert"><span>{actionError}</span><button on:click={onBackToItems}>返回列表</button></div>{/if}
  {#if data}
    {#if data.truncated}<p class="gantt-notice" role="status">当前显示 {data.entries.length} / {data.totalVisibleCount} 段区间、{data.unscheduled.length} / {data.totalUnscheduledCount} 条待核对事项。可缩小时间范围或筛选。</p>{/if}
    {#if rows.length > 0}
      <div class="gantt-viewport" bind:this={viewport} bind:clientHeight={viewportHeight} on:scroll={() => (scrollTop = viewport.scrollTop)} style={`--chart-width:${chartWidth}px`} aria-label="事项区间，左右方向键查看日期" role="region" aria-busy={loading}>
        <div class="gantt-sheet">
          <div class="gantt-axis">
            <div class="gantt-name-heading">事项 <span>创建 → 截止</span></div>
            <div class="gantt-ruler" aria-hidden="true">{#each data.ticks as tick}<span class:weekend={tick.weekend} style={`left:${tick.position}%`}>{tick.label}</span>{/each}{#if data.todayPosition !== null}<b class="gantt-now-label" style={`left:${data.todayPosition}%`}>今天</b>{/if}</div>
          </div>
          <div style={`height:${firstRow * 44}px`} aria-hidden="true"></div>
          {#each visibleRows as row, index (row.item.itemId)}
            <div class="gantt-row" class:completed={row.item.status === 'DONE'}>
              <button class="gantt-name" data-row-index={firstRow + index} tabindex={firstRow + index === focusRow ? 0 : -1} on:focus={() => (activeRow = firstRow + index)} on:keydown={(event) => navigateRows(event, firstRow + index)} on:click={() => openItem(row.item.itemId)} aria-label={`打开事项：${row.item.title}`}>
                <span class="gantt-status" class:overdue={row.item.overdue} aria-hidden="true">{row.item.status === 'DONE' ? '✓' : row.item.overdue ? '!' : '·'}</span>
                <span class="gantt-name-copy"><strong title={row.item.title}>{row.item.title}</strong><small>{eventKindLabel(row.item.eventKind)}{row.item.categoryName ? ` · ${row.item.categoryName}` : ''}{row.item.historyIncomplete ? ' · 历史未记录' : ''}</small></span>
              </button>
              <div class="gantt-track">
                {#each data.ticks as tick, tickIndex}<i class="gantt-gridline" class:weekend={tick.weekend} style={`left:${tick.position}%;width:${(data.ticks[tickIndex + 1]?.position ?? 100) - tick.position}%`} aria-hidden="true"></i>{/each}
                {#each row.segments as entry (entry.id)}
                  <button class="gantt-bar" class:overdue={entry.overdue} class:open-ended={entry.openEnded} class:clipped-start={entry.position.clippedStart} class:clipped-end={entry.position.clippedEnd} class:periodic={entry.shape === 'OCCURRENCE'} class:today-must={entry.eventKind === 'TODAY_MUST'} style={position(entry.position)} title={entryLabel(entry)} aria-label={entryLabel(entry)} tabindex="-1" on:click={() => openItem(entry.itemId)}>
                    <span class="gantt-state-layer" aria-hidden="true">
                      {#each entry.stateSegments as state}
                        <i class={`gantt-state ${state.kind.toLowerCase()}`} style={relativePosition(state.position, entry.position)} title={state.label}></i>
                      {/each}
                    </span>
                    {#if entry.deadlineMarker}<i class="gantt-marker deadline" style={relativeMarker(entry.deadlineMarker, entry.position)} title={entry.deadlineMarker.label} aria-hidden="true"></i>{/if}
                    {#if entry.completionMarker}<i class="gantt-marker completion" style={relativeMarker(entry.completionMarker, entry.position)} title={entry.completionMarker.label} aria-hidden="true"></i>{/if}
                    {#if entry.position.width >= 4}<span class="gantt-bar-label">{entry.openEnded ? '至今天' : entry.overdue ? '逾期至今天' : entry.endDate.slice(5).replace('-', '/')}</span>{/if}
                  </button>
                  {#if entry.highlight}<span class="gantt-highlight" class:warning={entry.shape === 'WARNING'} style={position(entry.highlight)} title={`${entry.shape === 'WARNING' ? '预警' : '活动'}区间：${entry.highlight.startLabel} 至 ${entry.highlight.endLabel}`}></span>{/if}
                {/each}
                {#if data.todayPosition !== null}<i class="gantt-now-line" style={`left:${data.todayPosition}%`} aria-hidden="true"></i>{/if}
              </div>
            </div>
          {/each}
          <div style={`height:${Math.max(0, rows.length - lastRow) * 44}px`} aria-hidden="true"></div>
        </div>
      </div>
      <footer class="gantt-legend"><span><i class="recorded"></i>已记录</span><span><i class="active"></i>进行中 / 今日必做</span><span><i class="completed"></i>已完成</span><span><i class="overdue"></i>逾期至今天</span><small>竖线为原截止 · 点击区间查看</small></footer>
    {:else if !loading && !error}
      <div class="gantt-empty"><h2>这段时间没有事项</h2><p>换一段时间，或清除筛选查看已有记录。</p><button class="secondary-button" on:click={resetFilters}>清除筛选</button><button class="secondary-button" on:click={onBackToItems}>返回列表</button></div>
    {/if}
    {#if data.unscheduled.length > 0}
      <section class="gantt-review" aria-labelledby="gantt-review-title"><h2 id="gantt-review-title">时间待核对 <span>{data.totalUnscheduledCount}</span></h2>{#each data.unscheduled as item}<button on:click={() => openItem(item.itemId)}><strong>{item.title}</strong><span>{item.reason} · 打开核对 →</span></button>{/each}</section>
    {/if}
  {:else if loading}<div class="gantt-loading" role="status">正在读取事项区间…</div>{/if}
</section>
