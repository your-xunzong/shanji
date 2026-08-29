<script lang="ts">
  import type { Item } from '../lib/types';

  export let items: Item[];
  export let busyItemId = '';
  export let onClose: () => void;
  export let onComplete: (item: Item) => Promise<void>;
  export let onSnooze: (item: Item, minutes: number) => Promise<void>;
  export let onAcknowledge: (item: Item) => Promise<void>;
  export let onOpen: (item: Item) => Promise<void>;

  let error = '';

  async function run(action: () => Promise<void>): Promise<void> {
    error = '';
    try {
      await action();
    } catch (cause) {
      error = cause instanceof Error && cause.message ? cause.message : '操作没有保存，请重试。';
    }
  }
</script>

<div class="panel-backdrop" role="presentation" on:click={onClose}></div>
<aside class="settings-panel reminder-center" aria-labelledby="reminder-center-heading">
  <header class="settings-header">
    <div><p>可能错过的提醒</p><h2 id="reminder-center-heading">待确认提醒</h2></div>
    <button class="icon-button" aria-label="关闭提醒中心" on:click={onClose}>
      <svg viewBox="0 0 20 20" aria-hidden="true"><path d="m5 5 10 10M15 5 5 15" /></svg>
    </button>
  </header>
  <div class="reminder-center-content">
    <p class="panel-intro">这里保留已经到期但尚未明确处理的事项。关闭通知或打开闪记不会自动清除。</p>
    {#if items.length === 0}
      <div class="reminder-center-empty"><strong>没有待确认提醒</strong><span>已完成、已暂停或明确确认的事项不会留在这里。</span></div>
    {:else}
      <div class="reminder-center-list">
        {#each items as item (item.id)}
          <article>
            <div><span>{item.completionPolicy === 'MUST_COMPLETE_TODAY' ? '今日必做' : '已到期'} · {item.dueLocalDate} {item.dueLocalTime}</span><h3>{item.title}</h3></div>
            <div class="reminder-center-actions">
              <button class="primary-mini" disabled={busyItemId === item.id} on:click={() => run(() => onComplete(item))}>完成</button>
              <button class="secondary-mini" disabled={busyItemId === item.id} on:click={() => run(() => onSnooze(item, 15))}>15 分钟后提醒</button>
              <button class="text-mini" disabled={busyItemId === item.id} on:click={() => run(() => onAcknowledge(item))}>确认已看到</button>
              <button class="text-mini" disabled={busyItemId === item.id} on:click={() => run(() => onOpen(item))}>打开</button>
            </div>
          </article>
        {/each}
      </div>
    {/if}
    {#if error}<p class="organizer-error" role="alert">{error}</p>{/if}
  </div>
</aside>
