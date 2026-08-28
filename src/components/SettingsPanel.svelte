<script lang="ts">
  import type { NotificationStatus, Settings, UpdateSettingsInput } from '../lib/types';

  export let settings: Settings;
  export let saving = false;
  export let onClose: () => void;
  export let onSave: (input: UpdateSettingsInput) => void;
  export let onTestNotification: () => Promise<void>;
  export let notificationStatus: NotificationStatus;
  export let onRegisterNotifications: () => Promise<void>;
  export let onUnregisterNotifications: () => Promise<void>;

  let form: Settings = structuredClone(settings);
  let updateExistingDefaultItems = false;
  let testingNotification = false;
  let notificationTestStatus = '';
  let changingNotificationIdentity = false;

  const weekdays = [
    { value: 1, label: '一' },
    { value: 2, label: '二' },
    { value: 3, label: '三' },
    { value: 4, label: '四' },
    { value: 5, label: '五' },
    { value: 6, label: '六' },
    { value: 7, label: '日' },
  ];

  function toggleWorkday(day: number): void {
    form.workdays = form.workdays.includes(day)
      ? form.workdays.filter((value) => value !== day)
      : [...form.workdays, day].sort();
    form = { ...form };
  }

  function save(): void {
    onSave({ ...form, updateExistingDefaultItems });
  }

  async function testNotification(): Promise<void> {
    if (testingNotification) return;
    testingNotification = true;
    notificationTestStatus = '正在创建测试事项…';
    try {
      await onTestNotification();
      notificationTestStatus = '测试事项已创建，约 10 秒后会弹出系统通知。';
    } catch (cause) {
      notificationTestStatus = cause instanceof Error && cause.message
        ? cause.message
        : '测试事项创建失败，请检查通知和数据目录设置。';
    } finally {
      testingNotification = false;
    }
  }

  async function changeNotificationIdentity(register: boolean): Promise<void> {
    if (changingNotificationIdentity) return;
    changingNotificationIdentity = true;
    notificationTestStatus = '';
    try {
      if (register) await onRegisterNotifications();
      else await onUnregisterNotifications();
    } catch (cause) {
      notificationTestStatus = cause instanceof Error && cause.message
        ? cause.message
        : '系统通知身份设置失败，请重试。';
    } finally {
      changingNotificationIdentity = false;
    }
  }

  function deliveryLabel(result: NotificationStatus['lastResult']): string {
    if (result === 'SUBMITTED') return '最近一次已提交给系统';
    if (result === 'FAILED') return '最近一次提交失败，后台会自动重试';
    if (result === 'DISABLED') return '最近一次因通知开关关闭而跳过';
    if (result === 'CLAIMED') return '最近一次正在提交';
    return '尚无投递记录';
  }
</script>

<div class="panel-backdrop" role="presentation" on:click={onClose}></div>
<aside class="settings-panel" aria-labelledby="settings-heading">
  <header class="settings-header">
    <div>
      <p class="eyebrow">后台设置</p>
      <h2 id="settings-heading">时间与提醒</h2>
    </div>
    <button class="icon-button" aria-label="关闭设置" on:click={onClose}>
      <svg viewBox="0 0 20 20" aria-hidden="true"><path d="m5 5 10 10M15 5 5 15" /></svg>
    </button>
  </header>

  <div class="settings-content">
    <section class="settings-section">
      <div class="section-heading">
        <h3>默认到期</h3>
        <p>未指定时间时使用。修改后默认只影响新事项和之后的自动顺延。</p>
      </div>

      <label class="form-field">
        <span>默认时间</span>
        <input type="time" bind:value={form.defaultDueTime} required />
      </label>

      <div class="form-field">
        <span>工作日</span>
        <div class="weekday-grid" aria-label="选择工作日">
          {#each weekdays as day}
            <button
              type="button"
              class:active={form.workdays.includes(day.value)}
              aria-pressed={form.workdays.includes(day.value)}
              on:click={() => toggleWorkday(day.value)}
            >{day.label}</button>
          {/each}
        </div>
      </div>

      <label class="check-row">
        <input type="checkbox" bind:checked={updateExistingDefaultItems} />
        <span>
          <strong>同时更新现有默认事项</strong>
          <small>保存前由后端校验；明确时间和今日必做事项不受影响。</small>
        </span>
      </label>
    </section>

    <section class="settings-section">
      <div class="section-heading">
        <h3>今日必做</h3>
        <p>到期后按此间隔继续提醒，直到完成或暂停。</p>
      </div>
      <label class="form-field">
        <span>默认提醒间隔</span>
        <select bind:value={form.overtimeIntervalMinutes}>
          <option value={15}>15 分钟</option>
          <option value={30}>30 分钟</option>
          <option value={60}>60 分钟</option>
          <option value={120}>2 小时</option>
        </select>
      </label>
    </section>

    <section class="settings-section">
      <label class="switch-row">
        <span>
          <strong>应用内勿扰</strong>
          <small>系统级勿扰始终由操作系统控制。</small>
        </span>
        <input class="switch" type="checkbox" bind:checked={form.quietHoursEnabled} />
      </label>
      {#if form.quietHoursEnabled}
        <div class="time-range">
          <label><span>开始</span><input type="time" bind:value={form.quietStart} /></label>
          <span aria-hidden="true">至</span>
          <label><span>结束</span><input type="time" bind:value={form.quietEnd} /></label>
        </div>
      {/if}
    </section>

    <section class="settings-section">
      <label class="form-field">
        <span>全局快捷键</span>
        <input type="text" bind:value={form.globalShortcut} spellcheck="false" />
      </label>
      <label class="switch-row">
        <span><strong>系统通知</strong><small>关闭后仍可在事项列表查看逾期状态。</small></span>
        <input class="switch" type="checkbox" bind:checked={form.notificationsEnabled} />
      </label>
      <div class="notification-status-card" data-ready={notificationStatus.canNotify}>
        <div>
          <strong>{notificationStatus.canNotify ? '原生系统通知已就绪' : '原生系统通知未就绪'}</strong>
          <small>{notificationStatus.message}</small>
          <small>{deliveryLabel(notificationStatus.lastResult)}</small>
        </div>
        {#if notificationStatus.portable && notificationStatus.identityStatus === 'registration_required'}
          <button
            class="secondary-button"
            disabled={changingNotificationIdentity}
            on:click={() => changeNotificationIdentity(true)}
          >{changingNotificationIdentity ? '正在启用…' : '启用便携版系统通知'}</button>
        {:else if notificationStatus.portable && notificationStatus.identityStatus === 'ready'}
          <button
            class="text-mini"
            disabled={changingNotificationIdentity}
            on:click={() => changeNotificationIdentity(false)}
          >撤销便携通知注册</button>
        {/if}
      </div>
      <div class="notification-test-row">
        <button
          class="secondary-button"
          disabled={testingNotification || !form.notificationsEnabled || !notificationStatus.canNotify}
          on:click={testNotification}
        >
          {testingNotification ? '正在准备…' : '10 秒后测试通知'}
        </button>
        {#if notificationTestStatus}
          <p role="status">{notificationTestStatus}</p>
        {/if}
      </div>
    </section>
  </div>

  <footer class="settings-footer">
    <button class="secondary-button" on:click={onClose}>取消</button>
    <button class="primary-button" disabled={saving || form.workdays.length === 0} on:click={save}>
      {saving ? '正在保存…' : '保存设置'}
    </button>
  </footer>
</aside>
