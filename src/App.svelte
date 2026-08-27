<script lang="ts">
  import CaptureView from './views/CaptureView.svelte';
  import DashboardView from './views/DashboardView.svelte';

  const view = new URLSearchParams(window.location.search).get('view');
</script>

{#if view === 'capture'}
  <CaptureView />
{:else}
  <DashboardView />
{/if}
