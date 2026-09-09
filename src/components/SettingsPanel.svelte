<script lang="ts">
  import { EVENT_KIND_OPTIONS, REMINDER_PLAN_OPTIONS } from '../lib/presentation';
  import type {
    AppInfo,
    AutostartStatus,
    NotificationStatus,
    Settings,
    UpdateSettingsInput,
    DataStatus,
    DataFileSummary,
    SmtpStatus,
    EventKind,
    ReminderPlan,
    UpdateState,
  } from '../lib/types';

  export let settings: Settings;
  export let saving = false;
  export let onClose: () => void;
  export let onSave: (input: UpdateSettingsInput) => Promise<void>;
  export let onTestNotification: () => Promise<void>;
  export let onTestReminderMode: (mode: 'persistent' | 'overlay') => Promise<string>;
  export let pendingReminderCount = 0;
  export let onOpenReminderCenter: () => void = () => {};
  export let smtpStatus: SmtpStatus = {
    passwordConfigured: false,
    verifiedAt: null,
    lastResult: null,
    lastErrorCode: null,
    lastAttemptAt: null,
  };
  export let onOpenEmailRules: () => void = () => {};
  export let onTestSmtp: () => Promise<string> = async () => {
    throw new Error('邮件测试暂时不可用。');
  };
  export let onRefreshSmtpStatus: () => Promise<SmtpStatus> = async () => smtpStatus;
  export let notificationStatus: NotificationStatus;
  export let onRegisterNotifications: () => Promise<void>;
  export let onUnregisterNotifications: () => Promise<void>;
  export let autostartStatus: AutostartStatus;
  export let onOpenOnboarding: () => void;
  export let dataStatus: DataStatus | null = null;
  export let onRefreshData: () => Promise<DataStatus> = async () => {
    throw new Error('暂时无法读取数据状态。');
  };
  export let onCreateBackup: () => Promise<DataFileSummary> = async () => {
    throw new Error('暂时无法创建备份。');
  };
  export let onOpenDataDirectory: () => Promise<void> = async () => {};
  export let onRestoreDatabase: (path: string) => Promise<void> = async () => {};
  export let appInfo: AppInfo | null = null;
  export let updateState: UpdateState | null = null;
  export let updateChecking = false;
  export let onSetAutoUpdate: (enabled: boolean) => Promise<UpdateState> = async () => {
    throw new Error('更新设置暂时不可用。');
  };
  export let onCheckForUpdates: () => Promise<void> = async () => {};
  export let onViewReleaseNotes: () => void = () => {};

  let form: Settings = structuredClone(settings);
  let updateExistingDefaultItems = false;
  let testingNotification = false;
  let notificationTestStatus = '';
  let changingNotificationIdentity = false;
  let saveError = '';
  let dataActionStatus = '';
  let dataActionBusy = false;
  let reminderModeTesting = '';
  let reminderModeStatus = '';
  let smtpPassword = '';
  let smtpTesting = false;
  let smtpActionStatus = '';
  let updatePreferenceBusy = false;
  let updateActionStatus = '';

  $: repeatTimesError = validateRepeatTimes(form.repeatDefaultTimes);

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

  function eventDefault(kind: EventKind): ReminderPlan {
    return form.eventKindDefaults.find((entry) => entry.eventKind === kind)?.reminderPlan ?? 'ONCE';
  }

  function setEventDefault(kind: EventKind, reminderPlan: ReminderPlan): void {
    form.eventKindDefaults = form.eventKindDefaults.map((entry) =>
      entry.eventKind === kind ? { ...entry, reminderPlan } : entry,
    );
    form = { ...form };
  }

  function validateRepeatTimes(values: string[]): string {
    const valid = (value: string) => /^(?:[01]\d|2[0-3]):[0-5]\d$/.test(value);
    if (values.length !== 2 || !valid(values[0] ?? '')) return '请设置有效的第一次重复提醒时间。';
    if (!valid(values[1] ?? '')) return '请设置有效的第二次重复提醒时间。';
    if (values[0] === values[1]) return '两次重复提醒不能使用相同时间。';
    return '';
  }

  function updateStatusLabel(state: UpdateState): string {
    if (!state.lastCheckedAt) return '尚未检查';
    const checkedAt = new Date(state.lastCheckedAt);
    const time = Number.isFinite(checkedAt.getTime())
      ? new Intl.DateTimeFormat('zh-CN', { month: 'numeric', day: 'numeric', hour: '2-digit', minute: '2-digit' }).format(checkedAt)
      : '时间未知';
    const result = state.lastCheckResult === 'UP_TO_DATE'
      ? '当前已是最新版本'
      : state.lastCheckResult === 'UPDATE_AVAILABLE'
        ? '发现可用更新'
        : '上次检查未完成';
    return `${time} · ${result}`;
  }

  async function toggleAutomaticUpdates(enabled: boolean): Promise<void> {
    if (updatePreferenceBusy) return;
    updatePreferenceBusy = true;
    updateActionStatus = '';
    try {
      updateState = await onSetAutoUpdate(enabled);
      updateActionStatus = enabled ? '已启用，每天最多检查一次。' : '已关闭，闪记不会自动联网检查。';
    } catch {
      updateActionStatus = '设置没有保存，自动检查状态保持原样。';
    } finally {
      updatePreferenceBusy = false;
    }
  }

  async function save(): Promise<void> {
    if (saving || repeatTimesError) return;
    saveError = '';
    try {
      await onSave({
        ...form,
        // 旧版全局重复字段只为数据兼容保留，新版频率由事项提醒方案决定。
        repeatUnacknowledgedEnabled: false,
        updateExistingDefaultItems,
        smtpPassword: smtpPassword || null,
      });
    } catch (cause) {
      saveError = cause instanceof Error && cause.message
        ? cause.message
        : typeof cause === 'string' && cause
          ? cause
          : '设置没有保存，请检查输入后重试。';
    }
  }

  async function testSmtp(): Promise<void> {
    if (smtpTesting) return;
    smtpTesting = true;
    smtpActionStatus = '正在连接邮件服务器并等待接收结果…';
    try {
      smtpActionStatus = await onTestSmtp();
      smtpStatus = await onRefreshSmtpStatus();
    } catch (cause) {
      smtpActionStatus = cause instanceof Error && cause.message
        ? cause.message
        : '测试邮件没有发送，请检查邮件设置。';
    } finally {
      smtpTesting = false;
    }
  }

  function smtpDeliveryLabel(status: SmtpStatus): string {
    if (status.lastResult === 'SUBMITTED') return '最近一次邮件已被服务器接受';
    if (status.lastResult === 'FAILED') {
      if (status.lastErrorCode === 'credential_store_unavailable') return '系统安全存储暂时不可用，请稍后重试';
      if (status.lastErrorCode === 'smtp_password_missing') return '邮件密码已缺失，请重新保存连接信息';
      if (status.lastErrorCode === 'smtp_configuration_invalid') return '邮件地址或服务器设置需要修改';
      return '最近一次连接失败，请检查网络和邮件服务器设置';
    }
    if (status.lastResult === 'CLAIMED') return '最近一次邮件正在发送';
    return '尚无邮件投递记录';
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

  async function testReminderMode(mode: 'persistent' | 'overlay'): Promise<void> {
    if (reminderModeTesting) return;
    reminderModeTesting = mode;
    reminderModeStatus = '正在准备测试…';
    try {
      reminderModeStatus = await onTestReminderMode(mode);
    } catch (cause) {
      reminderModeStatus = cause instanceof Error && cause.message
        ? cause.message
        : '提醒测试没有完成，请检查对应设置。';
    } finally {
      reminderModeTesting = '';
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

  function fileDate(value: string | null): string {
    if (!value) return '时间未知';
    return new Intl.DateTimeFormat('zh-CN', {
      year: 'numeric', month: '2-digit', day: '2-digit', hour: '2-digit', minute: '2-digit',
    }).format(new Date(value));
  }

  async function createBackup(): Promise<void> {
    if (dataActionBusy) return;
    dataActionBusy = true;
    dataActionStatus = '正在创建完整备份…';
    try {
      const backup = await onCreateBackup();
      dataStatus = await onRefreshData();
      dataActionStatus = `已备份 ${backup.itemCount} 项数据。`;
    } catch (cause) {
      dataActionStatus = cause instanceof Error && cause.message
        ? cause.message
        : '备份没有完成，现有数据未被修改。';
    } finally {
      dataActionBusy = false;
    }
  }

  async function restore(candidate: DataFileSummary): Promise<void> {
    const confirmed = window.confirm(
      `将恢复这份数据中的 ${candidate.itemCount} 项记录。闪记会先备份当前数据，然后重新启动。是否继续？`,
    );
    if (!confirmed || dataActionBusy) return;
    dataActionBusy = true;
    dataActionStatus = '正在准备恢复并重新启动…';
    try {
      await onRestoreDatabase(candidate.path);
    } catch (cause) {
      dataActionStatus = cause instanceof Error && cause.message
        ? cause.message
        : '数据没有恢复，当前数据保持不变。';
      dataActionBusy = false;
    }
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
        <h3>重复提醒默认时间</h3>
        <p>未指定时间的新重复事项每天使用两个时点。修改只影响之后新建、转换或恢复默认的事项。</p>
      </div>
      <div class="time-range repeat-time-range">
        <label><span>第一次</span><input type="time" bind:value={form.repeatDefaultTimes[0]} aria-invalid={!!repeatTimesError} /></label>
        <span aria-hidden="true">与</span>
        <label><span>第二次</span><input type="time" bind:value={form.repeatDefaultTimes[1]} aria-invalid={!!repeatTimesError} /></label>
      </div>
      {#if repeatTimesError}<p class="field-error" role="alert">{repeatTimesError}</p>{/if}
    </section>

    <section class="settings-section">
      <div class="section-heading">
        <h3>事件默认提醒</h3>
        <p>选择事件类型时自动使用；只影响新记录和之后主动转换的记录。</p>
      </div>
      <div class="event-default-grid">
        {#each EVENT_KIND_OPTIONS as option}
          <label>
            <span><strong>{option.label}</strong><small>{option.description}</small></span>
            <select value={eventDefault(option.value)} on:change={(event) => setEventDefault(option.value, event.currentTarget.value as ReminderPlan)}>
              {#each REMINDER_PLAN_OPTIONS as plan}<option value={plan.value}>{plan.label}</option>{/each}
            </select>
          </label>
        {/each}
      </div>
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
      <p class="field-help">需要至少一个修饰键和一个普通按键，例如 Ctrl+Shift+Space。</p>
      <label class="switch-row">
        <span>
          <strong>开机启动</strong>
          <small>{autostartStatus.message}</small>
        </span>
        <input
          class="switch"
          type="checkbox"
          bind:checked={form.autostartEnabled}
          disabled={!autostartStatus.available}
        />
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
      <div class="section-heading reminder-display-heading">
        <h3>提醒显示</h3>
        <p>提醒时间与频率由事件默认方案和单条事项决定；这里仅选择提醒从哪里出现。</p>
      </div>
      <div class="reminder-mode-list" aria-label="提醒显示方式">
        <div class="reminder-mode-row">
          <span class="reminder-source">系统</span>
          <label class="switch-row"><span><strong>持续提醒</strong><small>用于今日必做或“强制”方案；Windows 可直接完成或稍后提醒。不改变提醒频率。</small></span><input class="switch" type="checkbox" bind:checked={form.persistentNotificationsEnabled} /></label>
          <button class="text-mini" disabled={Boolean(reminderModeTesting)} on:click={() => testReminderMode('persistent')}>发送测试</button>
        </div>
        <div class="reminder-mode-row">
          <span class="reminder-source">闪记</span>
          <label class="switch-row"><span><strong>屏幕角落提醒窗</strong><small>每次到期时由闪记显示，不主动抢走输入焦点。不改变提醒频率。</small></span><input class="switch" type="checkbox" bind:checked={form.overlayRemindersEnabled} /></label>
          <button class="text-mini" disabled={Boolean(reminderModeTesting)} on:click={() => testReminderMode('overlay')}>发送测试</button>
        </div>
      </div>
      {#if reminderModeStatus}<p class="reminder-mode-status" role="status">{reminderModeStatus}</p>{/if}
      <div class="reminder-recovery-card">
        <div>
          <span class="reminder-recovery-eyebrow">错过提醒</span>
          <strong>待确认提醒 <span class="reminder-count">{pendingReminderCount} 条</span></strong>
          <small>到期后尚未完成、暂停或确认的事项会保留在这里，关闭通知不会清除。</small>
        </div>
        <button class="secondary-button" type="button" on:click={onOpenReminderCenter}>打开待确认提醒</button>
      </div>
      <button class="settings-guide-button" type="button" on:click={onOpenOnboarding}>
        重新打开首次引导 <span aria-hidden="true">→</span>
      </button>
    </section>

    <section class="settings-section smtp-section">
      <div class="section-heading">
        <h3>邮件发送连接</h3>
        <p>这里只负责连接你的邮箱。哪些事项发送、何时发送，在独立的“邮件提醒”工作台中设置。</p>
      </div>
      <label class="switch-row">
        <span><strong>启用 SMTP 邮件通知</strong><small>启用后需要填写安全连接和登录信息。</small></span>
        <input class="switch" type="checkbox" bind:checked={form.smtpEnabled} />
      </label>
      {#if form.smtpEnabled}
        <div class="smtp-grid">
          <label class="form-field smtp-host"><span>SMTP 服务器</span><input type="text" bind:value={form.smtpHost} placeholder="smtp.example.com" autocomplete="off" /></label>
          <label class="form-field"><span>端口</span><input type="number" min="1" max="65535" bind:value={form.smtpPort} /></label>
          <label class="form-field"><span>安全连接</span><select bind:value={form.smtpSecurity}><option value="tls">TLS（常用 465）</option><option value="starttls">STARTTLS（常用 587）</option></select></label>
          <label class="form-field"><span>发件人</span><input type="email" bind:value={form.smtpFrom} placeholder="me@example.com" autocomplete="email" /></label>
          <label class="form-field"><span>收件人</span><input type="email" bind:value={form.smtpTo} placeholder="me@example.com" autocomplete="email" /></label>
          <label class="form-field"><span>登录用户名</span><input type="text" bind:value={form.smtpUsername} placeholder="留空时使用发件人邮箱" autocomplete="username" /></label>
          <label class="form-field"><span>SMTP 密码</span><input type="password" bind:value={smtpPassword} placeholder={smtpStatus.passwordConfigured ? '已安全保存；留空表示不更改' : '请输入应用专用密码'} autocomplete="new-password" /></label>
        </div>
        <p class="field-help">密码只保存到操作系统凭据存储，不写入闪记数据库、日志或导出文件。</p>
        <div class="smtp-test-row">
          <div><strong>{smtpStatus.verifiedAt ? '发送连接已验证' : smtpStatus.passwordConfigured ? '密码已保存，连接待验证' : '尚未保存密码'}</strong><small>{smtpDeliveryLabel(smtpStatus)}</small></div>
          <button class="secondary-button" disabled={smtpTesting || !settings.smtpEnabled} on:click={testSmtp}>{smtpTesting ? '正在发送…' : '发送测试邮件'}</button>
        </div>
        {#if !settings.smtpEnabled}<p class="field-help">请先保存并重新打开设置，再发送测试邮件。</p>{/if}
        {#if smtpActionStatus}<p class="reminder-mode-status" role="status">{smtpActionStatus}</p>{/if}
        <button class="settings-guide-button" type="button" on:click={onOpenEmailRules}>打开邮件提醒规则 <span aria-hidden="true">→</span></button>
      {/if}
    </section>

    <section class="settings-section data-safety-section">
      <div class="section-heading">
        <h3>数据与备份</h3>
        <p>安装更新不会覆盖这里的数据。备份包含数据库中的事项、提醒计划和设置。</p>
      </div>
      {#if dataStatus}
        <div class="data-location-card">
          <strong>{dataStatus.current.itemCount} 项本地记录</strong>
          <span title={dataStatus.current.path}>{dataStatus.current.path}</span>
          <small>最近记录：{fileDate(dataStatus.current.updatedAt)} · {dataStatus.current.reminderHistoryCount} 条提醒历史</small>
        </div>
        <div class="data-action-row">
          <button class="secondary-button" disabled={dataActionBusy} on:click={createBackup}>
            {dataActionBusy ? '正在处理…' : '立即备份'}
          </button>
          <button class="text-mini" on:click={onOpenDataDirectory}>打开数据位置</button>
        </div>
        {#if dataStatus.latestBackup}
          <p class="data-backup-note">最近备份：{fileDate(dataStatus.latestBackup.updatedAt)}，共 {dataStatus.latestBackup.itemCount} 项</p>
        {/if}
        {#if dataStatus.recoveryCandidates.length > 0}
          <div class="recovery-candidates">
            <strong>找到可恢复的旧数据</strong>
            <p>恢复前会自动备份当前数据，不会合并两份记录。</p>
            {#each dataStatus.recoveryCandidates as candidate}
              <div>
                <span><b>{candidate.itemCount} 项</b><small>{fileDate(candidate.updatedAt)} · 已通过校验</small></span>
                <button class="text-mini" disabled={dataActionBusy} on:click={() => restore(candidate)}>恢复这份数据</button>
              </div>
            {/each}
          </div>
        {/if}
      {:else}
        <p class="data-backup-note">正在读取数据位置…</p>
      {/if}
      {#if dataActionStatus}<p class="data-action-status" role="status">{dataActionStatus}</p>{/if}
    </section>

    <section class="settings-section about-section">
      <div class="section-heading"><h3>关于闪记</h3><p>版本、更新与本地数据说明。</p></div>
      <div class="about-card">
        <span class="brand-mark large" aria-hidden="true"></span>
        <div><strong>{appInfo?.name ?? '闪记'}</strong><small>版本 {appInfo?.version ?? '读取中'}</small><small>{appInfo?.copyright ?? '© 2026 闪记'}</small></div>
      </div>
      {#if updateState}
        <div class="update-preference-card">
          <label>
            <span><strong>自动检查更新</strong><small>启用后每天最多连接 GitHub 一次，不会自动下载或安装。</small></span>
            <input
              type="checkbox"
              role="switch"
              checked={updateState.autoCheckEnabled}
              disabled={updatePreferenceBusy}
              on:change={(event) => void toggleAutomaticUpdates(event.currentTarget.checked)}
            />
          </label>
          <p>{updateStatusLabel(updateState)}</p>
          {#if updateActionStatus}<p class="update-action-status" role="status">{updateActionStatus}</p>{/if}
          <div class="data-action-row">
            <button class="secondary-button" disabled={updateChecking} on:click={onCheckForUpdates}>{updateChecking ? '正在检查…' : '立即检查更新'}</button>
            <button class="text-mini" on:click={onViewReleaseNotes}>查看本版更新说明</button>
          </div>
        </div>
      {/if}
      <p class="field-help">闪记默认离线运行。事项正文、备注和本地草稿不会在未授权时上传。</p>
      <div class="data-action-row"><button class="text-mini" on:click={onOpenDataDirectory}>打开数据位置</button></div>
      <details class="license-details"><summary>第三方开源许可</summary><p>闪记使用第三方开源组件。各组件版权归其权利人所有，分发包中的许可文件适用。</p></details>
    </section>
  </div>

  <footer class="settings-footer">
    {#if saveError}<p class="settings-save-error" role="alert">{saveError}</p>{/if}
    <button class="secondary-button" on:click={onClose}>取消</button>
    <button class="primary-button" disabled={saving || form.workdays.length === 0 || !!repeatTimesError} on:click={save}>
      {saving ? '正在保存…' : '保存设置'}
    </button>
  </footer>
</aside>
