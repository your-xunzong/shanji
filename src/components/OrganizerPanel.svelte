<script lang="ts">
  import type { Category, Tag, TaxonomyInput } from '../lib/types';

  export let categories: Category[];
  export let tags: Tag[];
  export let onClose: () => void;
  export let onCreateCategory: (input: TaxonomyInput) => Promise<void>;
  export let onUpdateCategory: (id: string, input: TaxonomyInput) => Promise<void>;
  export let onDeleteCategory: (id: string, reassignTo: string | null) => Promise<void>;
  export let onMoveCategory: (id: string, direction: 'up' | 'down') => Promise<void>;
  export let onCreateTag: (input: TaxonomyInput) => Promise<void>;
  export let onUpdateTag: (id: string, input: TaxonomyInput) => Promise<void>;
  export let onDeleteTag: (id: string) => Promise<void>;
  export let onMoveTag: (id: string, direction: 'up' | 'down') => Promise<void>;

  let categoryDrafts: Record<string, TaxonomyInput> = {};
  let tagDrafts: Record<string, TaxonomyInput> = {};
  let newCategory: TaxonomyInput = { name: '', color: '#627D98' };
  let newTag: TaxonomyInput = { name: '', color: '#B06C49' };
  let busy = '';
  let error = '';
  let deletingCategory: Category | null = null;
  let reassignDraft = '';

  $: for (const category of categories) {
    categoryDrafts[category.id] ??= { name: category.name, color: category.color };
  }
  $: for (const tag of tags) {
    tagDrafts[tag.id] ??= { name: tag.name, color: tag.color };
  }

  async function run(key: string, action: () => Promise<void>): Promise<void> {
    if (busy) return;
    busy = key;
    error = '';
    try {
      await action();
    } catch (cause) {
      error = cause instanceof Error && cause.message ? cause.message : '没有保存，请检查名称后重试。';
    } finally {
      busy = '';
    }
  }

  async function createCategory(): Promise<void> {
    if (!newCategory.name.trim()) return;
    await run('new-category', async () => {
      await onCreateCategory(newCategory);
      newCategory = { name: '', color: '#627D98' };
    });
  }

  async function createTag(): Promise<void> {
    if (!newTag.name.trim()) return;
    await run('new-tag', async () => {
      await onCreateTag(newTag);
      newTag = { name: '', color: '#B06C49' };
    });
  }

  function requestDeleteCategory(category: Category): void {
    deletingCategory = category;
    reassignDraft = '';
    error = '';
  }

  async function confirmDeleteCategory(): Promise<void> {
    if (!deletingCategory) return;
    const id = deletingCategory.id;
    await run(`delete-category-${id}`, () => onDeleteCategory(id, reassignDraft || null));
    if (!error) deletingCategory = null;
  }

  async function deleteTag(tag: Tag): Promise<void> {
    if (!window.confirm(`确定删除标签“${tag.name}”吗？它只会从事项上移除。`)) return;
    await run(`delete-tag-${tag.id}`, () => onDeleteTag(tag.id));
  }
</script>

<div class="panel-backdrop" role="presentation" on:click={onClose}></div>
<aside class="settings-panel organizer-panel" aria-labelledby="organizer-heading">
  <header class="settings-header">
    <div><p class="eyebrow">整理方式</p><h2 id="organizer-heading">类型与标签</h2></div>
    <button class="icon-button" aria-label="关闭类型与标签" on:click={onClose}>
      <svg viewBox="0 0 20 20" aria-hidden="true"><path d="m5 5 10 10M15 5 5 15" /></svg>
    </button>
  </header>

  <div class="settings-content">
    <section class="settings-section">
      <div class="section-heading"><h3>类型</h3><p>每个事项选择一个工作性质。没有类型的事项会显示在收件箱。</p></div>
      <div class="taxonomy-list">
        {#each categories as category, index (category.id)}
          <div class="taxonomy-row">
            <input type="color" bind:value={categoryDrafts[category.id].color} aria-label={`${category.name}的颜色`} />
            <div class="taxonomy-order"><button aria-label={`上移类型：${category.name}`} disabled={Boolean(busy) || index === 0} on:click={() => run(`move-category-${category.id}`, () => onMoveCategory(category.id, 'up'))}>↑</button><button aria-label={`下移类型：${category.name}`} disabled={Boolean(busy) || index === categories.length - 1} on:click={() => run(`move-category-${category.id}`, () => onMoveCategory(category.id, 'down'))}>↓</button></div>
            <input type="text" maxlength="30" bind:value={categoryDrafts[category.id].name} aria-label={`类型名称：${category.name}`} />
            <button class="text-mini" disabled={Boolean(busy)} on:click={() => run(`category-${category.id}`, () => onUpdateCategory(category.id, categoryDrafts[category.id]))}>保存</button>
            <button class="danger-mini" disabled={Boolean(busy)} on:click={() => requestDeleteCategory(category)}>删除</button>
          </div>
        {/each}
        {#if deletingCategory}
          <div class="taxonomy-delete-card">
            <strong>删除“{deletingCategory.name}”</strong>
            <p>事项不会被删除。请选择这些事项之后使用的类型。</p>
            <select bind:value={reassignDraft} aria-label="删除类型后的迁移目标">
              <option value="">改为收件箱（无类型）</option>
              {#each categories.filter((category) => category.id !== deletingCategory?.id) as category}
                <option value={category.id}>改为“{category.name}”</option>
              {/each}
            </select>
            <div><button class="danger-confirm" disabled={Boolean(busy)} on:click={confirmDeleteCategory}>确认删除</button><button class="text-mini" on:click={() => (deletingCategory = null)}>取消</button></div>
          </div>
        {/if}
        <div class="taxonomy-row taxonomy-new">
          <input type="color" bind:value={newCategory.color} aria-label="新类型颜色" />
          <input type="text" maxlength="30" bind:value={newCategory.name} placeholder="新类型名称" aria-label="新类型名称" />
          <button class="primary-mini" disabled={Boolean(busy) || !newCategory.name.trim()} on:click={createCategory}>添加类型</button>
        </div>
      </div>
    </section>

    <section class="settings-section">
      <div class="section-heading"><h3>标签</h3><p>标签可以多选，用颜色快速区分事项内容。</p></div>
      <div class="taxonomy-list">
        {#each tags as tag, index (tag.id)}
          <div class="taxonomy-row">
            <input type="color" bind:value={tagDrafts[tag.id].color} aria-label={`${tag.name}的颜色`} />
            <div class="taxonomy-order"><button aria-label={`上移标签：${tag.name}`} disabled={Boolean(busy) || index === 0} on:click={() => run(`move-tag-${tag.id}`, () => onMoveTag(tag.id, 'up'))}>↑</button><button aria-label={`下移标签：${tag.name}`} disabled={Boolean(busy) || index === tags.length - 1} on:click={() => run(`move-tag-${tag.id}`, () => onMoveTag(tag.id, 'down'))}>↓</button></div>
            <input type="text" maxlength="30" bind:value={tagDrafts[tag.id].name} aria-label={`标签名称：${tag.name}`} />
            <button class="text-mini" disabled={Boolean(busy)} on:click={() => run(`tag-${tag.id}`, () => onUpdateTag(tag.id, tagDrafts[tag.id]))}>保存</button>
            <button class="danger-mini" disabled={Boolean(busy)} on:click={() => deleteTag(tag)}>删除</button>
          </div>
        {/each}
        <div class="taxonomy-row taxonomy-new">
          <input type="color" bind:value={newTag.color} aria-label="新标签颜色" />
          <input type="text" maxlength="30" bind:value={newTag.name} placeholder="新标签名称" aria-label="新标签名称" />
          <button class="primary-mini" disabled={Boolean(busy) || !newTag.name.trim()} on:click={createTag}>添加标签</button>
        </div>
      </div>
      {#if tags.length === 0}<p class="taxonomy-empty">还没有标签。添加后可在事项编辑中选择多个标签。</p>{/if}
    </section>
    {#if error}<p class="organizer-error" role="alert">{error}</p>{/if}
  </div>
</aside>
