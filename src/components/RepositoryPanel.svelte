<script lang="ts">
  import { onMount } from 'svelte';
  import { isTauri } from '@tauri-apps/api/core';
  import { api } from '../lib/api';
  import type { RepositoryPreview, RepositoryStatus, RepositorySyncResult } from '../lib/types';

  export let onClose: () => void;
  export let onItemsChanged: () => Promise<void> = async () => {};
  export let onShowAllItems: () => Promise<void> = async () => {};

  let repository: RepositoryStatus | null = null;
  let preview: RepositoryPreview | null = null;
  let result: RepositorySyncResult | null = null;
  let loading = true;
  let busy = '';
  let error = '';
  let retryChoose = false;
  let retrySnapshot = false;
  let statusMessage = '';

  onMount(() => void loadStatus());

  async function loadStatus(): Promise<void> {
    loading = true;
    error = '';
    retryChoose = false;
    retrySnapshot = false;
    try {
      repository = await api.getRepositoryStatus();
    } catch (cause) {
      error = readableError(cause, '个人仓库状态暂时无法读取。本机记录不受影响。');
    } finally {
      loading = false;
    }
  }

  async function chooseRepository(): Promise<void> {
    if (busy) return;
    busy = 'configure';
    error = '';
    retryChoose = false;
    retrySnapshot = false;
    statusMessage = '';
    let selectionComplete = false;
    try {
      // 原生选择器也可能失败；取消和异常都必须解除忙碌状态。
      let selected: string | string[] | null = 'D:\\闪记个人仓库';
      if (isTauri()) {
        const { open } = await import('@tauri-apps/plugin-dialog');
        selected = await open({ directory: true, multiple: false, title: '选择个人仓库所在文件夹' });
      }
      if (!selected || Array.isArray(selected)) return;
      if (!confirm(`闪记将在以下位置建立独立的“shanji-repository”文件夹：\n\n${selected}\n\n不会移动或替换当前本地数据库。继续吗？`)) return;
      selectionComplete = true;
      repository = await api.configureDataRepository(selected);
      preview = null;
      result = null;
      if (repository.snapshotReady) {
        statusMessage = `个人仓库已建立，已写入 ${repository.localItemCount} 条本机记录。`;
      } else {
        retrySnapshot = true;
        error = '个人仓库位置已保存，但本机快照尚未写入。本机记录没有改变，请重新写入。';
      }
    } catch (cause) {
      retryChoose = true;
      error = readableError(cause, selectionComplete
        ? '个人仓库没有建立。本机记录仍保存在原位置，可以换一个文件夹重试。'
        : '文件夹选择窗口未能打开。本机记录和仓库设置没有改变，请重试。');
    } finally {
      busy = '';
    }
  }

  async function publishSnapshot(): Promise<void> {
    if (busy) return;
    busy = 'snapshot';
    error = '';
    statusMessage = '';
    try {
      repository = await api.publishDataRepositorySnapshot();
      retrySnapshot = false;
      statusMessage = `已向个人仓库写入 ${repository.localItemCount} 条本机记录。`;
    } catch {
      retrySnapshot = true;
      error = '本机快照没有写入个人仓库。本机记录仍然安全，请检查仓库空间和权限后重试。';
      repository = await api.getRepositoryStatus().catch(() => repository);
    } finally {
      busy = '';
    }
  }

  async function previewMerge(): Promise<void> {
    if (busy) return;
    busy = 'preview';
    error = '';
    retryChoose = false;
    retrySnapshot = false;
    statusMessage = '';
    try {
      preview = await api.previewRepositoryMerge();
      result = null;
    } catch (cause) {
      error = readableError(cause, '仓库内容暂时无法预览。本机记录没有改变。');
    } finally {
      busy = '';
    }
  }

  async function syncRepository(): Promise<void> {
    if (!preview || busy) return;
    const impact = preview.conflicts > 0
      ? `其中 ${preview.conflicts} 项存在不同修改，将保留为带“冲突副本”标记的两条记录。`
      : '没有发现需要人工选择的冲突。';
    if (!confirm(`即将汇入 ${preview.newItems} 条新记录。${impact}\n\n同步前会先创建本地恢复备份，完成后更新仓库中的本机快照。继续吗？`)) return;
    busy = 'sync';
    error = '';
    retryChoose = false;
    retrySnapshot = false;
    try {
      result = await api.syncDataRepository();
      repository = await api.getRepositoryStatus();
      preview = null;
      await onItemsChanged();
      if (result.packageWritten) {
        statusMessage = result.importedItems > 0
          ? `已汇入 ${result.importedItems} 条记录，并更新个人仓库中的本机快照。`
          : '仓库已更新，本机没有需要汇入的新记录。';
      } else {
        retrySnapshot = true;
        statusMessage = `已向本机汇入 ${result.importedItems} 条记录，但归并后的仓库快照尚未写入。`;
      }
    } catch (cause) {
      error = readableError(cause, '同步没有完成。本机原有记录和同步前备份仍然保留，可以重试。');
    } finally {
      busy = '';
    }
  }

  async function disableRepository(): Promise<void> {
    if (busy || !confirm('停止使用这个个人仓库？仓库文件和本机记录都不会被删除。')) return;
    busy = 'disable';
    error = '';
    retryChoose = false;
    retrySnapshot = false;
    try {
      repository = await api.disableDataRepository();
      preview = null;
      result = null;
      statusMessage = '已停止使用个人仓库，闪记继续使用本机数据。';
    } catch (cause) {
      error = readableError(cause, '暂时无法停用个人仓库，请重试。');
    } finally {
      busy = '';
    }
  }

  function readableError(_cause: unknown, fallback: string): string {
    return fallback;
  }

  function formatDate(value: string | null): string {
    if (!value) return '尚未同步';
    return new Intl.DateTimeFormat('zh-CN', { month: 'numeric', day: 'numeric', hour: '2-digit', minute: '2-digit' }).format(new Date(value));
  }
</script>

<div class="panel-backdrop" role="presentation" on:click={(event) => event.currentTarget === event.target && onClose()}>
  <div class="side-panel repository-panel" role="dialog" aria-modal="true" aria-labelledby="repository-title">
    <header class="panel-header">
      <div><p class="eyebrow">本地资料流</p><h2 id="repository-title">个人仓库</h2><p>用你选择的本地或同步盘文件夹，在自己的设备间汇入记录。</p></div>
      <button class="icon-button" aria-label="关闭个人仓库" on:click={onClose}>×</button>
    </header>

    {#if error}<div class="inline-error compact" role="alert"><span>{error}</span><button disabled={Boolean(busy)} on:click={retryChoose ? chooseRepository : retrySnapshot ? publishSnapshot : loadStatus}>{retryChoose ? '重新选择文件夹' : retrySnapshot ? '重新写入' : '重试'}</button></div>{/if}
    {#if statusMessage}<p class="panel-status" role="status">{statusMessage}</p>{/if}

    {#if loading}
      <div class="panel-loading" aria-label="正在读取个人仓库"><span></span><span></span><span></span></div>
    {:else if repository}
      <div class="repository-flow" aria-label="个人仓库数据路径">
        <div class="repository-node local"><span aria-hidden="true">●</span><strong>这台设备</strong><small>{repository.localItemCount} 条记录</small></div>
        <div class="repository-flow-line"><i></i><span>写入快照 · 预览后汇入</span><i></i></div>
        <div class:unavailable={repository.configured && !repository.available} class="repository-node shared"><span aria-hidden="true">◇</span><strong>{repository.configured ? '个人仓库' : '等待选择位置'}</strong><small>{repository.configured ? `${repository.packageCount} 个设备包` : '不会改变本机数据位置'}</small></div>
      </div>

      {#if !repository.configured}
        <div class="repository-intro"><p>适合把同步盘、移动硬盘或局域网目录作为交换点。闪记写入独立数据包，不会直接共用正在运行的数据库。</p><ul><li>同步前先显示新增、重复、冲突与删除数量</li><li>冲突内容保留两份，不静默覆盖</li><li>仓库不可用时继续使用本机记录</li></ul><button class="primary-button" disabled={Boolean(busy)} on:click={chooseRepository}>选择仓库位置</button></div>
      {:else}
        <div class:unavailable={!repository.available} class="repository-location-card">
          <div><span class="connection-dot" aria-hidden="true"></span><strong>{repository.available ? '仓库可用' : '仓库暂时不可用'}</strong></div>
          <code title={repository.path ?? ''}>{repository.path}</code>
          <small>{repository.available ? repository.snapshotReady ? `本机快照：${formatDate(repository.lastPackageAt)} · 上次汇入：${formatDate(repository.lastSyncedAt)}` : '位置已保存，本机快照尚未写入。' : '本机记录照常保存；重新连接此位置后再同步。'}</small>
          <div class="data-action-row"><button class="text-mini" disabled={!repository.available} on:click={() => api.openRepositoryDirectory()}>打开仓库</button><button class="text-mini" on:click={chooseRepository}>更换位置</button><button class="text-mini danger-text" on:click={disableRepository}>停止使用</button></div>
          {#if repository.available && !repository.snapshotReady}<button class="secondary-button repository-snapshot-button" disabled={Boolean(busy)} on:click={publishSnapshot}>{busy === 'snapshot' ? '正在写入…' : '写入本机快照'}</button>{/if}
        </div>

        <div class="repository-summary-grid">
          <div><strong>{repository.pendingSourceCount}</strong><span>待汇入来源</span></div><div><strong>{repository.conflictCount}</strong><span>待查看冲突</span></div><div><strong>{repository.packageCount}</strong><span>仓库数据包</span></div>
        </div>

        <button class="primary-button repository-preview-button" disabled={Boolean(busy) || !repository.available} on:click={previewMerge}>{busy === 'preview' ? '正在检查…' : '检查其他设备的数据'}</button>

        {#if preview}
          <section class="merge-preview" aria-labelledby="merge-preview-title">
            <div><p class="eyebrow">尚未改变本机记录</p><h3 id="merge-preview-title">本次同步预览</h3></div>
            <div class="merge-counts"><span><strong>{preview.newItems}</strong> 新增</span><span><strong>{preview.unchangedItems}</strong> 已有</span><span class:has-conflict={preview.conflicts > 0}><strong>{preview.conflicts}</strong> 冲突</span><span><strong>{preview.tombstones}</strong> 删除记录</span></div>
            {#each preview.sources as source}
              <div class="repository-source"><div><strong>{source.sourceInstanceId === '另一台设备' ? source.sourceInstanceId : `设备 ${source.sourceInstanceId.slice(0, 8)}`}</strong><small>{formatDate(source.exportedAt)} · 共 {source.itemCount} 条</small></div><span>+{source.newItems} / 冲突 {source.conflicts}</span></div>
            {/each}
            {#if preview.conflicts > 0}<p class="conflict-note">冲突不会被覆盖：同步后会保留原记录，并新增带“冲突副本”标记的记录供你整理。</p>{/if}
            {#if preview.settingsNeedReview}<p class="conflict-note">发现其他设备的设置。本机已有事项，因此设置不会自动替换。</p>{/if}
            {#if preview.sources.length === 0}<p class="conflict-note">没有发现其他设备的新数据。继续后只更新这台设备的仓库快照。</p>{/if}
            <button class="primary-button" disabled={Boolean(busy)} on:click={syncRepository}>{busy === 'sync' ? '正在备份并汇入…' : preview.sources.length === 0 ? '更新本机快照' : '汇入并更新仓库'}</button>
          </section>
        {/if}

        {#if result}
          <section class="sync-result" aria-live="polite"><span aria-hidden="true">{result.packageWritten ? '✓' : '!'}</span><div><strong>{result.packageWritten ? '同步完成' : '本机已汇入，仓库快照待写入'}</strong><p>汇入 {result.importedItems} 条，保留 {result.conflicts} 个冲突副本，应用 {result.tombstonesApplied} 条删除记录。</p><small>完成于 {formatDate(result.syncedAt)}</small>{#if !result.packageWritten}<button class="text-mini" on:click={publishSnapshot}>重新写入仓库快照</button>{/if}{#if result.importedItems > 0}<button class="text-mini" on:click={onShowAllItems}>查看全部事项</button>{/if}</div></section>
        {/if}
      {/if}
    {/if}
  </div>
</div>
