<script lang="ts">
  import { onMount } from 'svelte';
  import CaptureView from './views/CaptureView.svelte';
  import DashboardView from './views/DashboardView.svelte';
  import DataRecoveryView from './views/DataRecoveryView.svelte';
  import ReminderView from './views/ReminderView.svelte';
  import { api } from './lib/api';
  import type { StartupStatus } from './lib/types';

  const view = new URLSearchParams(window.location.search).get('view');
  const secondaryWindow = view === 'capture' || view === 'reminder';
  let startupStatus: StartupStatus | null = secondaryWindow
    ? {
        mode: 'ready',
        targetPath: '',
        message: '',
        current: null,
        recoveryCandidates: [],
        diagnostic: 'secondary_window=ready',
      }
    : null;
  let startupError = '';

  onMount(async () => {
    if (secondaryWindow) return;
    try {
      startupStatus = await api.getStartupStatus();
    } catch {
      startupError = '无法检查本机数据。闪记没有继续打开，请重新启动后再试。';
    }
  });
</script>

{#if startupError}
  <main class="startup-loading" role="alert">
    <span class="brand-mark" aria-hidden="true"></span>
    <strong>本机数据检查没有完成</strong>
    <p>{startupError}</p>
  </main>
{:else if !startupStatus}
  <main class="startup-loading" role="status" aria-live="polite">
    <span class="brand-mark" aria-hidden="true"></span>
    <strong>正在检查本机数据…</strong>
  </main>
{:else if startupStatus.mode !== 'ready'}
  <DataRecoveryView
    status={startupStatus}
    onRestore={api.restoreStartupDatabase}
    onOpenLocation={api.openStartupDataDirectory}
    onRetry={api.retryStartup}
  />
{:else if view === 'capture'}
  <CaptureView />
{:else if view === 'reminder'}
  <ReminderView />
{:else}
  <DashboardView />
{/if}
