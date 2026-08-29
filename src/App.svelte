<script lang="ts">
  import CaptureView from './views/CaptureView.svelte';
  import DashboardView from './views/DashboardView.svelte';
  import ReminderView from './views/ReminderView.svelte';

  const view = new URLSearchParams(window.location.search).get('view');
</script>

{#if view === 'capture'}
  <CaptureView />
{:else if view === 'reminder'}
  <ReminderView />
{:else}
  <DashboardView />
{/if}
