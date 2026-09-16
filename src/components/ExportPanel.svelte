<script lang="ts">
  import { EVENT_KIND_OPTIONS } from '../lib/presentation';
  import { api } from '../lib/api';
  import type {
    Category,
    EventKind,
    ExportFilterInput,
    SummaryPeriod,
    SummaryRequest,
    SummaryTemplatePreview,
    Tag,
  } from '../lib/types';

  export let categories: Category[];
  export let tags: Tag[];
  export let exporting = false;
  export let onClose: () => void;
  export let onExport: (filter: ExportFilterInput) => Promise<void>;
  export let onChooseSummaryTemplate: () => Promise<string | null> = async () => null;
  export let onGenerateSummary: (request: SummaryRequest) => Promise<void> = async () => {};

  const today = new Date();
  let mode: 'excel' | 'summary' = 'excel';
  let status: ExportFilterInput['status'] = 'all';
  let eventKind: EventKind | '' = '';
  let categoryId = '';
  let tagId = '';
  let createdFrom = '';
  let createdTo = '';
  let period: SummaryPeriod = 'MONTH';
  let year = today.getFullYear();
  let month = today.getMonth() + 1;
  let quarter = Math.floor(today.getMonth() / 3) + 1;
  let templatePath = '';
  let templatePreview: SummaryTemplatePreview | null = null;
  let validating = false;
  let error = '';

  function localBoundary(date: string, endOfDay: boolean): string | null {
    if (!date) return null;
    const value = new Date(`${date}T${endOfDay ? '23:59:59.999' : '00:00:00.000'}`);
    return Number.isNaN(value.getTime()) ? null : value.toISOString();
  }

  async function submitExcel(): Promise<void> {
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

  async function chooseTemplate(): Promise<void> {
    if (exporting || validating) return;
    const selected = await onChooseSummaryTemplate();
    if (!selected) return;
    templatePath = selected;
    templatePreview = null;
    error = '';
    validating = true;
    try {
      templatePreview = await api.validateSummaryTemplate(selected);
      if (!templatePreview.valid) error = templatePreview.message;
    } catch {
      error = '模板无法读取，原文件没有改变。请检查文件是否为有效的 .docx 文档。';
    } finally {
      validating = false;
    }
  }

  async function submitSummary(): Promise<void> {
    if (!templatePreview?.valid) {
      error = '请先选择并通过检查的 Word 模板。';
      return;
    }
    error = '';
    await onGenerateSummary({
      period,
      year,
      month: period === 'MONTH' ? month : null,
      quarter: period === 'QUARTER' ? quarter : null,
      templatePath,
      outputPath: '',
    });
  }
</script>

<div class="panel-backdrop" role="presentation" on:click={onClose}></div>
<aside class="settings-panel export-panel" aria-labelledby="export-heading">
  <header class="settings-header">
    <div><p>生成本地资料</p><h2 id="export-heading">导出与小结</h2></div>
    <button class="icon-button" aria-label="关闭导出设置" on:click={onClose}>
      <svg viewBox="0 0 20 20" aria-hidden="true"><path d="m5 5 10 10M15 5 5 15" /></svg>
    </button>
  </header>

  <div class="export-tabs" role="tablist" aria-label="输出类型">
    <button class:active={mode === 'excel'} role="tab" aria-selected={mode === 'excel'} on:click={() => { mode = 'excel'; error = ''; }}>Excel 明细</button>
    <button class:active={mode === 'summary'} role="tab" aria-selected={mode === 'summary'} on:click={() => { mode = 'summary'; error = ''; }}>Word 小结</button>
  </div>

  <div class="export-content">
    {#if mode === 'excel'}
      <p class="panel-intro">选择需要的事项。报表分别保留事件类型、类型和标签，不会修改现有数据。</p>
      <div class="export-grid">
        <label><span>事项状态</span><select bind:value={status}><option value="all">全部（不含回收站）</option><option value="open">待处理</option><option value="done">已完成</option></select></label>
        <label><span>事件类型</span><select bind:value={eventKind}><option value="">全部事件类型</option>{#each EVENT_KIND_OPTIONS as option}<option value={option.value}>{option.label}</option>{/each}</select></label>
        <label><span>类型</span><select bind:value={categoryId}><option value="">全部类型</option>{#each categories as category}<option value={category.id}>{category.name}</option>{/each}</select></label>
        <label><span>标签</span><select bind:value={tagId}><option value="">全部标签</option>{#each tags as tag}<option value={tag.id}>{tag.name}</option>{/each}</select></label>
        <label><span>创建日期从</span><input type="date" bind:value={createdFrom} /></label>
        <label><span>到</span><input type="date" bind:value={createdTo} /></label>
      </div>
      <div class="export-note"><strong>文件内容提示</strong><span>事项正文和备注可能包含私人信息，请把导出文件保存到安全位置。</span></div>
    {:else}
      <p class="panel-intro">选择周期和你自己的 Word 模板。闪记只替换正文或表格中的占位符，不会改动模板。</p>
      <div class="summary-period-grid">
        <label><span>小结周期</span><select bind:value={period}><option value="MONTH">月度</option><option value="QUARTER">季度</option></select></label>
        <label><span>年份</span><input type="number" min="2000" max="2200" bind:value={year} /></label>
        {#if period === 'MONTH'}
          <label><span>月份</span><select bind:value={month}>{#each Array(12) as _, index}<option value={index + 1}>{index + 1} 月</option>{/each}</select></label>
        {:else}
          <label><span>季度</span><select bind:value={quarter}>{#each Array(4) as _, index}<option value={index + 1}>第 {index + 1} 季度</option>{/each}</select></label>
        {/if}
      </div>

      <section class="template-card" aria-labelledby="template-title">
        <div><strong id="template-title">Word 模板</strong><small>{templatePath || '尚未选择 .docx 模板'}</small></div>
        <button class="secondary-button" disabled={exporting || validating} on:click={chooseTemplate}>{validating ? '正在检查…' : templatePath ? '更换模板' : '选择模板'}</button>
        {#if templatePreview?.valid}<p class="template-ok" role="status">✓ {templatePreview.message}</p>{/if}
      </section>

      <details class="placeholder-guide">
        <summary>模板里的占位符放在哪里？</summary>
        <p>单值占位符放在普通正文中，例如 <code>{'{{report_title}}'}</code>、<code>{'{{period_summary}}'}</code>。列表占位符放在 Word 表格的一行里，这一行会按事项数量自动复制。</p>
        <div><span>统计</span><code>{'{{total_count}}  {{completed_count}}  {{completion_rate}}'}</code></div>
        <div><span>未完成事项表格行</span><code>{'{{open_item.title}}  {{open_item.due_at}}  {{open_item.status}}'}</code></div>
        <div><span>已完成事项表格行</span><code>{'{{completed_item.title}}  {{completed_item.completed_at}}'}</code></div>
        <p>占位符必须连续输入在正文或表格单元格中；暂不支持页眉、页脚和文本框。</p>
      </details>
      <div class="export-note"><strong>隐私与失败恢复</strong><span>全部处理都在本机完成。模板无效或写入失败时，模板和已有输出文件保持不变。</span></div>
    {/if}
    {#if error}<p class="organizer-error" role="alert">{error}</p>{/if}
  </div>

  <footer class="settings-footer">
    <button class="secondary-button" disabled={exporting} on:click={onClose}>取消</button>
    {#if mode === 'excel'}
      <button class="primary-button" disabled={exporting} on:click={submitExcel}>{exporting ? '正在导出…' : '选择位置并导出'}</button>
    {:else}
      <button class="primary-button" disabled={exporting || !templatePreview?.valid} on:click={submitSummary}>{exporting ? '正在生成…' : '选择位置并生成'}</button>
    {/if}
  </footer>
</aside>
