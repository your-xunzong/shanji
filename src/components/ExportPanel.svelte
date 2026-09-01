<script lang="ts">
  import { EVENT_KIND_OPTIONS } from '../lib/presentation';
  import type { Category, EventKind, ExportFilterInput, Tag } from '../lib/types';

  export let categories: Category[];
  export let tags: Tag[];
  export let exporting = false;
  export let onClose: () => void;
  export let onExport: (filter: ExportFilterInput) => Promise<void>;

  let status: ExportFilterInput['status'] = 'all';
  let eventKind: EventKind | '' = '';
  let categoryId = '';
  let tagId = '';
  let createdFrom = '';
  let createdTo = '';
  let error = '';

  function localBoundary(date: string, endOfDay: boolean): string | null {
    if (!date) return null;
    const value = new Date(`${date}T${endOfDay ? '23:59:59.999' : '00:00:00.000'}`);
    return Number.isNaN(value.getTime()) ? null : value.toISOString();
  }

  async function submit(): Promise<void> {
    if (createdFrom && createdTo && createdFrom > createdTo) {
      error = '开始日期不能晚于结束日期';
      return;
    }
    error = '';
    await onExport({
      status,
      eventKind: eventKind || null,
      categoryId: categoryId || null,
      tagId: tagId || null,
      createdFrom: localBoundary(createdFrom, false),
      createdTo: localBoundary(createdTo, true),
    });
  }
</script>

<div class="panel-backdrop" role="presentation" on:click={onClose}></div>
<aside class="settings-panel export-panel" aria-labelledby="export-heading">
  <header class="settings-header">
    <div><p>生成本地报表</p><h2 id="export-heading">导出 Excel</h2></div>
    <button class="icon-button" aria-label="关闭导出设置" on:click={onClose}>
      <svg viewBox="0 0 20 20" aria-hidden="true"><path d="m5 5 10 10M15 5 5 15" /></svg>
    </button>
  </header>

  <div class="export-content">
    <p class="panel-intro">选择需要的事项。报表分别保留事件类型、类型和标签，不会修改现有数据。</p>
    <div class="export-grid">
      <label><span>事项状态</span><select bind:value={status}><option value="all">全部（不含回收站）</option><option value="open">待处理</option><option value="done">已完成</option></select></label>
      <label><span>事件类型</span><select bind:value={eventKind}><option value="">全部事件类型</option>{#each EVENT_KIND_OPTIONS as option}<option value={option.value}>{option.label}</option>{/each}</select></label>
      <label><span>类型</span><select bind:value={categoryId}><option value="">全部类型</option>{#each categories as category}<option value={category.id}>{category.name}</option>{/each}</select></label>
      <label><span>标签</span><select bind:value={tagId}><option value="">全部标签</option>{#each tags as tag}<option value={tag.id}>{tag.name}</option>{/each}</select></label>
      <label><span>创建日期从</span><input type="date" bind:value={createdFrom} /></label>
      <label><span>到</span><input type="date" bind:value={createdTo} /></label>
    </div>
    {#if error}<p class="organizer-error" role="alert">{error}</p>{/if}
    <div class="export-note"><strong>文件内容提示</strong><span>事项正文和备注可能包含私人信息，请把导出文件保存到安全位置。</span></div>
  </div>

  <footer class="settings-footer">
    <button class="secondary-button" disabled={exporting} on:click={onClose}>取消</button>
    <button class="primary-button" disabled={exporting} on:click={submit}>{exporting ? '正在导出…' : '选择位置并导出'}</button>
  </footer>
</aside>
