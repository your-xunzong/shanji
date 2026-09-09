<script lang="ts">
  import { onMount } from 'svelte';
  import type { AppUpdate, UpdateDownloadProgress, UpdatePhase } from '../lib/updater';

  export let phase: UpdatePhase;
  export let update: AppUpdate | null = null;
  export let currentVersion: string;
  export let currentNotes: string[] = [];
  export let progress: UpdateDownloadProgress = {
    downloadedBytes: 0,
    totalBytes: null,
    percent: null,
    finished: false,
  };
  export let error = '';
  export let installMode: 'AUTOMATIC' | 'DOWNLOAD_ONLY' = 'AUTOMATIC';
  export let onClose: () => void;
  export let onRetry: () => Promise<void>;
  export let onDownload: () => Promise<void>;
  export let onCancelDownload: () => Promise<void>;
  export let onInstall: () => Promise<void>;
  export let onSnooze: () => Promise<void>;
  export let onOpenRelease: () => Promise<void>;
  let closeButton: HTMLButtonElement;
  let panelElement: HTMLDivElement;

  $: targetVersion = update?.version ?? currentVersion;
  $: notes = update?.notes ?? currentNotes;
  $: progressLabel = progress.totalBytes
    ? `${formatBytes(progress.downloadedBytes)} / ${formatBytes(progress.totalBytes)}`
    : `${formatBytes(progress.downloadedBytes)} 已下载`;

  function formatBytes(bytes: number): string {
    if (bytes < 1024) return `${bytes} B`;
    if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
    return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
  }

  function releaseDate(value: string | null | undefined): string {
    if (!value) return '发布日期以 GitHub Release 为准';
    const parsed = new Date(value);
    if (!Number.isFinite(parsed.getTime())) return '发布日期以 GitHub Release 为准';
    return new Intl.DateTimeFormat('zh-CN', { year: 'numeric', month: 'long', day: 'numeric' }).format(parsed);
  }

  onMount(() => closeButton?.focus());

  function handleKeydown(event: KeyboardEvent): void {
    if (event.key === 'Escape' && phase !== 'downloading' && phase !== 'installing' && !event.isComposing) {
      event.preventDefault();
      onClose();
      return;
    }
    if (event.key === 'Tab' && panelElement) {
      const controls = [...panelElement.querySelectorAll<HTMLElement>('button:not(:disabled), [href], input:not(:disabled)')];
      const first = controls[0];
      const last = controls.at(-1);
      if (!first || !last) return;
      if (event.shiftKey && document.activeElement === first) {
        event.preventDefault();
        last.focus();
      } else if (!event.shiftKey && document.activeElement === last) {
        event.preventDefault();
        first.focus();
      }
    }
  }
</script>

<svelte:window on:keydown={handleKeydown} />

<div class="panel-backdrop update-backdrop" role="presentation" on:click={phase === 'downloading' || phase === 'installing' ? undefined : onClose}></div>
<div bind:this={panelElement} class="update-panel" role="dialog" aria-modal="true" aria-labelledby="update-title">
  <header class="update-panel-header">
    <div>
      <p class="eyebrow">版本更新</p>
      <h2 id="update-title">
        {#if phase === 'current'}本版更新说明{:else if phase === 'upToDate'}当前已是最新版本{:else}更新到 v{targetVersion}{/if}
      </h2>
    </div>
    <button bind:this={closeButton} class="icon-close" aria-label="关闭更新面板" disabled={phase === 'installing'} on:click={onClose}>×</button>
  </header>

  <div class="update-panel-body">
    {#if phase === 'checking'}
      <div class="update-state" role="status">
        <span class="update-spinner" aria-hidden="true"></span>
        <strong>正在连接 GitHub 检查新版本…</strong>
        <p>这不会上传事项、备注或本地路径。</p>
      </div>
    {:else if phase === 'upToDate'}
      <div class="update-state update-state-success" role="status">
        <span class="version-node verified" aria-hidden="true">✓</span>
        <strong>v{currentVersion} 已是最新稳定版</strong>
        <p>你可以关闭此面板继续使用闪记。</p>
      </div>
    {:else if phase === 'failed'}
      <div class="update-state update-state-error" role="alert">
        <span class="version-node failed" aria-hidden="true">!</span>
        <strong>暂时无法完成更新操作</strong>
        <p>{error || '当前版本和本机数据没有改变，请稍后重试。'}</p>
      </div>
    {:else}
      {#if phase === 'current'}
        <div class="current-release-version"><span class="version-node verified" aria-hidden="true">✓</span><span><b>v{currentVersion}</b><small>当前正在使用</small></span></div>
      {:else}
        <div class="version-track" aria-label={`当前版本 v${currentVersion}，目标版本 v${targetVersion}`}>
          <span class="version-stop current"><b>v{currentVersion}</b><small>当前版本</small></span>
          <span class="track-line" class:active={phase === 'downloaded' || phase === 'installing'}>
            {#if phase === 'downloading'}
              <i style={`width:${progress.percent ?? 12}%`}></i>
            {:else if phase === 'downloaded' || phase === 'installing'}
              <i style="width:100%"></i>
            {/if}
          </span>
          <span class="version-stop target" class:verified={phase === 'downloaded' || phase === 'installing'}><b>v{targetVersion}</b><small>签名更新</small></span>
        </div>
      {/if}

      {#if update && phase !== 'current'}
        <p class="release-date">{releaseDate(update.date)}</p>
      {/if}

      <section class="release-notes" aria-labelledby="release-notes-title">
        <h3 id="release-notes-title">这次更新</h3>
        <ul>
          {#each notes as note}<li>{note}</li>{/each}
        </ul>
      </section>

      {#if phase === 'downloading'}
        <div class="download-progress" role="status" aria-live="polite">
          <div><strong>正在下载经过签名的更新包</strong><span>{progress.percent === null ? progressLabel : `${Math.round(progress.percent)}% · ${progressLabel}`}</span></div>
          <progress max="100" value={progress.percent ?? undefined}></progress>
          <p>取消下载不会更改当前程序或事项数据。</p>
        </div>
      {:else if phase === 'downloaded'}
        <p class="install-note" role="status">更新包已下载并通过签名验证。安装时闪记会关闭并重新启动，请先完成正在编辑的设置。</p>
      {:else if phase === 'installing'}
        <p class="install-note" role="status">正在安装更新并准备重新启动，请不要关闭电脑。</p>
      {:else if phase === 'cancelled'}
        <p class="install-note" role="status">下载已取消，当前版本和本机数据没有改变。</p>
      {:else if installMode === 'DOWNLOAD_ONLY' && phase !== 'current'}
        <p class="install-note">当前安装方式不支持在应用内替换程序。闪记会打开官方发布页，由你手动下载安装包。</p>
      {/if}
    {/if}
  </div>

  <footer class="update-panel-footer">
    {#if phase === 'checking' || phase === 'installing'}
      <span></span>
    {:else if phase === 'failed'}
      <button class="text-button" on:click={onOpenRelease}>打开发布页</button>
      <button class="primary-button" on:click={onRetry}>重新检查</button>
    {:else if phase === 'upToDate' || phase === 'current'}
      <button class="text-button" on:click={onOpenRelease}>打开发布页</button>
      <button class="primary-button" on:click={onClose}>关闭说明</button>
    {:else if phase === 'downloading'}
      <span></span>
      <button class="secondary-button" on:click={onCancelDownload}>取消下载</button>
    {:else if phase === 'downloaded'}
      <span></span>
      <button class="primary-button" on:click={onInstall}>安装并重启</button>
    {:else}
      <button class="text-button" on:click={onSnooze}>24 小时后提醒</button>
      {#if installMode === 'DOWNLOAD_ONLY'}
        <button class="primary-button" on:click={onOpenRelease}>打开官方下载页</button>
      {:else}
        <button class="primary-button" on:click={onDownload}>下载更新</button>
      {/if}
    {/if}
  </footer>
</div>

<style>
  .update-backdrop { z-index: 64; }
  .update-panel {
    position: fixed;
    z-index: 65;
    inset: 0 0 0 auto;
    width: min(460px, calc(100vw - 28px));
    display: grid;
    grid-template-rows: auto minmax(0, 1fr) auto;
    background: var(--surface, #fff);
    border-left: 1px solid var(--line, #d6dee7);
    box-shadow: -18px 0 44px rgb(34 51 73 / 14%);
  }
  .update-panel-header, .update-panel-footer { display: flex; align-items: center; justify-content: space-between; gap: 16px; padding: 22px 24px; }
  .update-panel-header { border-bottom: 1px solid var(--line, #d6dee7); }
  .update-panel-header h2 { margin: 3px 0 0; font-family: var(--font-ui); font-size: 23px; line-height: 1.25; color: var(--ink, #223349); }
  .update-panel-body { overflow-y: auto; padding: 24px; }
  .update-panel-footer { border-top: 1px solid var(--line, #d6dee7); min-height: 76px; }
  .icon-close { width: 38px; height: 38px; border: 0; border-radius: 50%; background: transparent; color: var(--muted, #66758a); font-size: 25px; cursor: pointer; }
  .icon-close:hover { background: var(--surface-muted, #eef2f6); }
  .version-track { display: grid; grid-template-columns: auto minmax(48px, 1fr) auto; align-items: center; gap: 10px; margin: 4px 0 12px; }
  .current-release-version { display:flex; align-items:center; gap:10px; margin:4px 0 20px; }
  .current-release-version > span:last-child { display:grid; gap:2px; }
  .current-release-version b { font-family:var(--font-mono); font-size:14px; }
  .current-release-version small { color:var(--muted, #66758a); font-size:12px; }
  .version-stop { display: grid; gap: 3px; min-width: 88px; }
  .version-stop.target { text-align: right; }
  .version-stop b { font-family: var(--font-mono); font-size: 14px; color: var(--ink, #223349); }
  .version-stop small, .release-date { color: var(--muted, #66758a); font-size: 12px; }
  .track-line { position: relative; height: 3px; overflow: hidden; background: var(--line, #d6dee7); }
  .track-line::before, .track-line::after { content: ''; position: absolute; z-index: 2; top: -3px; width: 9px; height: 9px; border: 2px solid var(--primary, #315e91); border-radius: 50%; background: var(--surface, #fff); }
  .track-line::before { left: 0; } .track-line::after { right: 0; }
  .track-line i { display: block; height: 100%; background: var(--primary, #315e91); transition: width 160ms ease; }
  .track-line.active::after { border-color: var(--success, #397565); background: var(--success, #397565); }
  .release-date { margin: 0 0 24px; }
  .release-notes { padding: 20px 0; border-top: 1px solid var(--line, #d6dee7); }
  .release-notes h3 { margin: 0 0 12px; font-family: var(--font-ui); font-size: 16px; color: var(--ink, #223349); }
  .release-notes ul { display: grid; gap: 11px; margin: 0; padding-left: 20px; }
  .release-notes li { padding-left: 4px; color: var(--ink-soft, #42536a); font-size: 14px; line-height: 1.6; }
  .download-progress, .install-note { margin: 18px 0 0; padding: 14px 16px; border: 1px solid var(--line, #d6dee7); border-left: 3px solid var(--primary, #315e91); background: var(--surface-muted, #f7f9fc); color: var(--ink-soft, #42536a); font-size: 13px; line-height: 1.55; }
  .download-progress > div { display: flex; justify-content: space-between; gap: 12px; }
  .download-progress span, .download-progress p { color: var(--muted, #66758a); font-size: 12px; }
  .download-progress p { margin: 8px 0 0; }
  progress { width: 100%; height: 6px; margin-top: 12px; accent-color: var(--primary, #315e91); }
  .update-state { display: grid; justify-items: start; gap: 8px; padding: 24px 0; }
  .update-state strong { font-size: 16px; color: var(--ink, #223349); }
  .update-state p { margin: 0; color: var(--muted, #66758a); font-size: 13px; line-height: 1.55; }
  .update-spinner, .version-node { width: 34px; height: 34px; display: grid; place-items: center; border-radius: 50%; }
  .update-spinner { border: 3px solid var(--line, #d6dee7); border-top-color: var(--primary, #315e91); animation: update-spin 800ms linear infinite; }
  .version-node.verified { background: color-mix(in srgb, var(--success, #397565) 12%, transparent); color: var(--success, #397565); }
  .version-node.failed { background: color-mix(in srgb, var(--danger, #b84040) 10%, transparent); color: var(--danger, #b84040); }
  .text-button { border: 0; background: transparent; color: var(--primary, #315e91); font: inherit; font-size: 13px; cursor: pointer; }
  @keyframes update-spin { to { transform: rotate(360deg); } }
  @media (prefers-reduced-motion: reduce) { .update-spinner { animation: none; border-color: var(--primary, #315e91); } .track-line i { transition: none; } }
  @media (forced-colors: active) { .update-panel, .release-notes, .download-progress, .install-note { border-color: CanvasText; } .track-line, .track-line i { background: CanvasText; } }
</style>
