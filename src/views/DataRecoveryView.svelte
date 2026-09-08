<script lang="ts">
  import type { DataFileSummary, StartupStatus } from '../lib/types';

  export let status: StartupStatus;
  export let onRestore: (path: string) => Promise<void>;
  export let onOpenLocation: (path: string) => Promise<void>;
  export let onRetry: () => Promise<void>;

  let busyPath = '';
  let retrying = false;
  let actionError = '';
  let copyStatus = '';

  function fileDate(value: string | null): string {
    if (!value) return '更新时间未知';
    return new Intl.DateTimeFormat('zh-CN', {
      year: 'numeric',
      month: '2-digit',
      day: '2-digit',
      hour: '2-digit',
      minute: '2-digit',
    }).format(new Date(value));
  }

  function summaryDetails(candidate: DataFileSummary): string[] {
    const details = [`${candidate.reminderHistoryCount} 条提醒历史`];
    if (candidate.hasCustomSettings) details.push('个性设置已找到');
    else if (candidate.hasSettings) details.push('设置已找到');
    if (candidate.hasDraft) details.push('未提交草稿已找到');
    return details;
  }

  async function restore(candidate: DataFileSummary): Promise<void> {
    if (busyPath || retrying) return;
    const confirmed = window.confirm(
      `将恢复这份数据中的 ${candidate.itemCount} 项记录。当前数据文件会保留在备份中，随后闪记将重新启动。是否继续？`,
    );
    if (!confirmed) return;
    busyPath = candidate.path;
    actionError = '';
    try {
      await onRestore(candidate.path);
    } catch (cause) {
      actionError = cause instanceof Error && cause.message
        ? cause.message
        : '原数据没有恢复，现有文件仍然保留。请重试或打开数据位置检查。';
      busyPath = '';
    }
  }

  async function openLocation(path: string): Promise<void> {
    actionError = '';
    try {
      await onOpenLocation(path);
    } catch (cause) {
      actionError = cause instanceof Error && cause.message
        ? cause.message
        : '无法打开数据位置，请展开诊断信息后手动打开。';
    }
  }

  async function retry(): Promise<void> {
    if (busyPath || retrying) return;
    retrying = true;
    actionError = '';
    try {
      await onRetry();
    } catch (cause) {
      actionError = cause instanceof Error && cause.message
        ? cause.message
        : '重新检查没有完成，原数据仍然保留。';
      retrying = false;
    }
  }

  async function copyDiagnostic(): Promise<void> {
    copyStatus = '';
    try {
      await navigator.clipboard.writeText(status.diagnostic);
      copyStatus = '诊断信息已复制';
    } catch {
      copyStatus = '无法自动复制，请选中下方内容手动复制';
    }
  }
</script>

<main class="recovery-shell">
  <section class="recovery-workspace" aria-labelledby="recovery-title">
    <header class="recovery-brand">
      <span class="brand-mark" aria-hidden="true"></span>
      <span>闪记</span>
    </header>

    <div class="recovery-heading">
      <p class="eyebrow">本机数据检查</p>
      <h1 id="recovery-title">
        {status.mode === 'recovery_required' ? '找到原来的数据' : '原数据仍在'}
      </h1>
      <p>{status.message}</p>
    </div>

    {#if status.recoveryCandidates.length > 0}
      <div class="continuity-list" aria-label="可恢复的数据">
        {#each status.recoveryCandidates as candidate, index}
          <article class="continuity-source">
            <div class="continuity-rail" aria-hidden="true">
              <span></span><span></span><span></span>
            </div>
            <div class="continuity-copy">
              <small>原数据 {status.recoveryCandidates.length > 1 ? index + 1 : ''}</small>
              <strong>{candidate.itemCount} 项记录</strong>
              <p>{fileDate(candidate.updatedAt)}</p>
              <ul aria-label="数据检查结果">
                <li>已通过完整性与版本检查</li>
                {#each summaryDetails(candidate) as detail}<li>{detail}</li>{/each}
              </ul>
            </div>
            <button
              class="primary-button recovery-primary"
              disabled={Boolean(busyPath) || retrying}
              on:click={() => restore(candidate)}
            >
              {busyPath === candidate.path ? '正在恢复原数据…' : '恢复原数据并重新启动'}
            </button>
            <button
              class="text-mini"
              disabled={Boolean(busyPath) || retrying}
              on:click={() => openLocation(candidate.path)}
            >打开原数据位置</button>
          </article>
        {/each}
      </div>
    {:else}
      <div class="continuity-blocked" role="alert">
        <span class="continuity-stop" aria-hidden="true"></span>
        <div>
          <strong>闪记没有创建新数据</strong>
          <p>请先检查磁盘空间、文件权限或应用版本。处理后可重新检查。</p>
        </div>
      </div>
    {/if}

    {#if actionError}<p class="recovery-error" role="alert">{actionError}</p>{/if}

    <div class="recovery-secondary-actions">
      <button class="secondary-button" disabled={Boolean(busyPath) || retrying} on:click={retry}>
        {retrying ? '正在重新检查…' : '重新检查'}
      </button>
      <button class="text-mini" disabled={Boolean(busyPath) || retrying} on:click={() => openLocation(status.targetPath)}>
        打开当前数据位置
      </button>
    </div>

    <details class="recovery-diagnostics">
      <summary>诊断信息</summary>
      <p>以下内容不包含事项正文、备注或凭据。</p>
      <dl>
        <div><dt>当前位置</dt><dd>{status.targetPath}</dd></div>
        {#each status.recoveryCandidates as candidate}
          <div><dt>原数据位置</dt><dd>{candidate.path}</dd></div>
        {/each}
      </dl>
      <code>{status.diagnostic}</code>
      <div class="diagnostic-actions">
        <button class="text-mini" on:click={copyDiagnostic}>复制诊断信息</button>
        {#if copyStatus}<span role="status">{copyStatus}</span>{/if}
      </div>
    </details>
  </section>
</main>
