<script lang="ts">
  import { onMount } from 'svelte';
  import type {
    AutostartStatus,
    NotificationStatus,
    OnboardingFinishInput,
    Settings,
  } from '../lib/types';

  export let settings: Settings;
  export let notificationStatus: NotificationStatus;
  export let autostartStatus: AutostartStatus;
  export let firstRun: boolean;
  export let onFinish: (input: OnboardingFinishInput) => Promise<void>;
  export let onSkip: () => Promise<void>;

  let step = 0;
  let autostartEnabled = firstRun ? true : autostartStatus.enabled;
  let defaultDueTime = settings.defaultDueTime;
  let repeatTimeFirst = settings.repeatDefaultTimes[0] ?? '10:00';
  let repeatTimeSecond = settings.repeatDefaultTimes[1] ?? '17:00';
  let globalShortcut = settings.globalShortcut;
  let enablePortableNotifications = false;
  let busy = false;
  let error = '';
  let dialogElement: HTMLDivElement;

  const steps = [
    { label: '通知', caption: '到点，由系统提醒' },
    { label: '启动', caption: '登录后静静待命' },
    { label: '时间', caption: '按你的节奏提醒' },
    { label: '快捷键', caption: '随时写下一句' },
  ];
  const titles = ['提醒交给系统', '需要时，它已经在', '选好默认提醒时间', '留一个顺手的入口'];
  const quickTimes = ['17:30', '18:00', '18:30'];

  $: shortcutError = validateShortcut(globalShortcut);
  $: timeError = validateTimes(defaultDueTime, repeatTimeFirst, repeatTimeSecond);

  onMount(() => {
    const previousFocus = document.activeElement as HTMLElement | null;
    dialogElement.querySelector<HTMLElement>('button:not([disabled]), input:not([disabled])')?.focus();
    return () => previousFocus?.focus();
  });

  function validateShortcut(value: string): string {
    const trimmed = value.trim();
    if (!trimmed) return '快捷键不能为空，请输入类似 Ctrl+Shift+Space 的组合。';
    const modifiers = new Set([
      'alt', 'option', 'ctrl', 'control', 'shift', 'command', 'cmd',
      'super', 'meta', 'commandorcontrol',
    ]);
    const tokens = trimmed.split('+').map((token) => token.trim());
    if (tokens.some((token) => !token)) {
      return '快捷键格式无效，请输入类似 Ctrl+Shift+Space 的组合。';
    }
    if (tokens.every((token) => modifiers.has(token.toLowerCase()))) {
      return '快捷键必须包含一个普通按键，例如 Space、M 或 F8。';
    }
    if (tokens.length < 2) return '快捷键必须同时包含修饰键和一个普通按键。';
    return '';
  }

  function validateTimes(defaultTime: string, firstTime: string, secondTime: string): string {
    const valid = (value: string) => /^(?:[01]\d|2[0-3]):[0-5]\d$/.test(value);
    if (!valid(defaultTime)) return '请选择有效的默认到期时间。';
    if (!valid(firstTime)) return '请选择有效的第一次重复提醒时间。';
    if (!valid(secondTime)) return '请选择有效的第二次重复提醒时间。';
    if (firstTime === secondTime) return '两次重复提醒不能使用相同时间。';
    return '';
  }

  function next(): void {
    error = '';
    if (step === 2 && timeError) return;
    if (step < steps.length - 1) step += 1;
  }

  async function finish(): Promise<void> {
    if (busy || shortcutError || timeError) return;
    busy = true;
    error = '';
    try {
      await onFinish({
        autostartEnabled,
        defaultDueTime,
        repeatDefaultTimes: [repeatTimeFirst, repeatTimeSecond].sort(),
        globalShortcut: globalShortcut.trim(),
        enablePortableNotifications,
      });
    } catch (cause) {
      error = cause instanceof Error && cause.message
        ? cause.message
        : '设置没有完成，请根据提示调整后重试。';
    } finally {
      busy = false;
    }
  }

  async function skip(): Promise<void> {
    if (busy) return;
    busy = true;
    error = '';
    try {
      await onSkip();
    } catch (cause) {
      error = cause instanceof Error && cause.message
        ? cause.message
        : '无法关闭首次引导，请重试。';
    } finally {
      busy = false;
    }
  }

  function handleDialogKeydown(event: KeyboardEvent): void {
    if (event.key === 'Escape') {
      event.preventDefault();
      void skip();
      return;
    }
    if (event.key !== 'Tab') return;
    const focusable = [...dialogElement.querySelectorAll<HTMLElement>(
      'button:not([disabled]), input:not([disabled]), select:not([disabled]), textarea:not([disabled])',
    )];
    if (focusable.length === 0) return;
    const first = focusable[0];
    const last = focusable[focusable.length - 1];
    if (event.shiftKey && document.activeElement === first) {
      event.preventDefault();
      last.focus();
    } else if (!event.shiftKey && document.activeElement === last) {
      event.preventDefault();
      first.focus();
    }
  }
</script>

<div class="onboarding-backdrop"></div>
<div
  class="onboarding-dialog"
  role="dialog"
  tabindex="-1"
  aria-modal="true"
  aria-labelledby="onboarding-title"
  bind:this={dialogElement}
  on:keydown={handleDialogKeydown}
>
  <aside class="onboarding-rail" aria-label="设置进度">
    <div class="onboarding-brand">
      <span class="brand-mark" aria-hidden="true"></span>
      <strong>闪记</strong>
    </div>
    <ol>
      {#each steps as item, index}
        <li class:active={step === index} class:complete={step > index} aria-current={step === index ? 'step' : undefined}>
          <span class="rail-node">{step > index ? '✓' : index + 1}</span>
          <div><strong>{item.label}</strong><small>{item.caption}</small></div>
        </li>
      {/each}
    </ol>
  </aside>

  <div class="onboarding-sheet">
    <header>
      <p class="eyebrow">把提醒方式设置好</p>
      <h2 id="onboarding-title">{titles[step]}</h2>
    </header>

    <div class="onboarding-body">
      {#if step === 0}
        <p class="onboarding-lead">闪记使用操作系统原生横幅。窗口隐藏或驻留托盘时，到点依然会提醒。</p>
        <div class="setup-card" data-state={notificationStatus.canNotify ? 'ready' : 'attention'}>
          <span class="setup-signal" aria-hidden="true"></span>
          <div>
            <strong>{notificationStatus.canNotify ? '原生系统通知已就绪' : '需要启用系统通知身份'}</strong>
            <p>{notificationStatus.message}</p>
          </div>
        </div>
        {#if notificationStatus.portable && !notificationStatus.canNotify}
          <label class="onboarding-choice">
            <input type="checkbox" bind:checked={enablePortableNotifications} />
            <span><strong>启用便携版系统通知</strong><small>完成设置后在当前用户范围注册，可随时从后台设置撤销。</small></span>
          </label>
        {/if}
        <p class="onboarding-note">Windows 专注模式可能把横幅收进通知中心，闪记不会绕过系统勿扰设置。</p>
      {:else if step === 1}
        <p class="onboarding-lead">登录后让闪记安静地驻留后台，不显示主窗口，也不打断当前工作。</p>
        <label class="onboarding-switch-card">
          <span>
            <strong>登录后自动启动闪记</strong>
            <small>只有点击“完成设置”后才会写入系统。</small>
          </span>
          <input class="switch" type="checkbox" bind:checked={autostartEnabled} disabled={!autostartStatus.available} />
        </label>
        <div class="setup-explanation">
          <span aria-hidden="true">↳</span>
          <p>{autostartStatus.message}</p>
        </div>
        {#if autostartStatus.portable}
          <p class="onboarding-note warning">便携版启动项指向当前程序路径。移动或删除整个目录后，需要重新设置。</p>
        {/if}
      {:else if step === 2}
        <p class="onboarding-lead">设置未指定时间的到期点，以及重复事项每天的两个提醒时点。已有事项不会改变。</p>
        <div class="onboarding-time-card">
          <label class="onboarding-time-field">
            <span>未指定时间的默认到期</span>
            <input
              type="time"
              bind:value={defaultDueTime}
              required
              aria-invalid={!!timeError}
              aria-describedby={timeError ? 'onboarding-time-error' : undefined}
            />
          </label>
          <div class="onboarding-time-choices">
            <span>常用时间</span>
            <div>
              {#each quickTimes as time}
                <button
                  type="button"
                  class:active={defaultDueTime === time}
                  aria-pressed={defaultDueTime === time}
                  on:click={() => (defaultDueTime = time)}
                >{time}</button>
              {/each}
            </div>
          </div>
        </div>
        <div class="onboarding-repeat-times" aria-labelledby="onboarding-repeat-label">
          <div>
            <strong id="onboarding-repeat-label">重复事项每天提醒两次</strong>
            <small>新事项会复制这两个时间，以后修改默认值不会改动已有事项。</small>
          </div>
          <div class="paired-time-fields">
            <label><span>第一次</span><input type="time" bind:value={repeatTimeFirst} aria-invalid={!!timeError} /></label>
            <span aria-hidden="true">·</span>
            <label><span>第二次</span><input type="time" bind:value={repeatTimeSecond} aria-invalid={!!timeError} /></label>
          </div>
        </div>
        {#if timeError}
          <p id="onboarding-time-error" class="onboarding-field-error" role="alert">{timeError}</p>
        {:else}
          <p class="reminder-time-preview">普通未指定时间的事项在 <strong>{defaultDueTime}</strong> 到期；重复事项默认在 <strong>{[repeatTimeFirst, repeatTimeSecond].sort().join('、')}</strong> 提醒。</p>
        {/if}
      {:else}
        <p class="onboarding-lead">这个组合键只负责唤起闪记。应用不会记录其他按键或输入内容。</p>
        <label class="onboarding-shortcut-field">
          <span>全局快捷键</span>
          <input bind:value={globalShortcut} spellcheck="false" autocomplete="off" />
        </label>
        {#if shortcutError}
          <p class="onboarding-field-error" role="alert">{shortcutError}</p>
        {:else}
          <div class="shortcut-preview" aria-label={`当前快捷键 ${globalShortcut}`}>
            {#each globalShortcut.split('+') as key}<kbd>{key.trim()}</kbd>{/each}
          </div>
        {/if}
        <p class="onboarding-note">若组合键已被其他程序占用，原快捷键会继续生效，不会留下半保存状态。</p>
      {/if}
    </div>

    {#if error}<div class="onboarding-error" role="alert">{error}</div>{/if}

    <footer>
      <button class="onboarding-skip" disabled={busy} on:click={skip}>{firstRun ? '暂时跳过' : '关闭'}</button>
      <div>
        {#if step > 0}<button class="secondary-button" disabled={busy} on:click={() => (step -= 1)}>上一步</button>{/if}
        {#if step < steps.length - 1}
          <button class="primary-button" disabled={busy || (step === 2 && !!timeError)} on:click={next}>继续</button>
        {:else}
          <button class="primary-button" disabled={busy || !!shortcutError || !!timeError} on:click={finish}>
            {busy ? '正在设置…' : '完成设置'}
          </button>
        {/if}
      </div>
    </footer>
  </div>
</div>
