<script lang="ts">
  import { onMount } from 'svelte';
  import { api } from '../lib/api';
  import { EVENT_KIND_OPTIONS, eventKindLabel } from '../lib/presentation';
  import type {
    Category,
    EmailDeliveryRule,
    EmailDeliveryRuleInput,
    EmailDeliveryStrategy,
    EmailRuleDimension,
    Settings,
    SmtpStatus,
    Tag,
  } from '../lib/types';

  export let categories: Category[] = [];
  export let tags: Tag[] = [];
  export let settings: Settings;
  export let smtpStatus: SmtpStatus;
  export let onClose: () => void;
  export let onOpenSettings: () => void;

  let rules: EmailDeliveryRule[] = [];
  let loading = true;
  let saving = false;
  let error = '';
  let status = '';
  let editing = false;
  let form: EmailDeliveryRuleInput = freshRule();

  const dimensionOptions: { value: EmailRuleDimension; label: string }[] = [
    { value: 'EVENT_KIND', label: '事件类型' },
    { value: 'CATEGORY', label: '分类' },
    { value: 'TAG', label: '标签' },
  ];
  const strategyOptions: { value: EmailDeliveryStrategy; label: string; help: string }[] = [
    { value: 'FIRST_DUE', label: '首次到期时', help: '每个事项首次到期时发送一次' },
    { value: 'DAILY_FIRST', label: '每天首次到期时', help: '同一事项每天最多发送一次' },
    { value: 'EACH_PLAN', label: '每次计划提醒时', help: '每次命中提醒计划时都发送' },
    { value: 'DAILY_DIGEST', label: '每日汇总', help: '当天命中的事项合并为一封邮件' },
  ];

  $: matchOptions = form.matchDimension === 'EVENT_KIND'
    ? EVENT_KIND_OPTIONS.map((option) => ({ value: option.value, label: option.label }))
    : form.matchDimension === 'CATEGORY'
      ? categories.map((category) => ({ value: category.id, label: category.name }))
      : tags.map((tag) => ({ value: tag.id, label: tag.name }));

  onMount(() => void loadRules());

  function freshRule(): EmailDeliveryRuleInput {
    return {
      id: null,
      matchDimension: 'EVENT_KIND',
      matchValue: 'ORDINARY',
      strategy: 'FIRST_DUE',
      recipient: settings?.smtpTo ?? '',
      digestLocalTime: '18:00',
      enabled: Boolean(smtpStatus?.verifiedAt),
    };
  }

  async function loadRules(): Promise<void> {
    loading = true;
    error = '';
    try {
      rules = await api.listEmailDeliveryRules();
    } catch (cause) {
      error = readableError(cause, '邮件提醒规则暂时无法读取，请稍后重试。');
    } finally {
      loading = false;
    }
  }

  function beginCreate(): void {
    form = freshRule();
    editing = true;
    error = '';
    status = '';
  }

  function beginEdit(rule: EmailDeliveryRule): void {
    form = { ...rule };
    editing = true;
    error = '';
    status = '';
  }

  function setDimension(value: string): void {
    const matchDimension = value as EmailRuleDimension;
    const matchValue = matchDimension === 'EVENT_KIND'
      ? EVENT_KIND_OPTIONS[0]?.value ?? ''
      : matchDimension === 'CATEGORY'
        ? categories[0]?.id ?? ''
        : tags[0]?.id ?? '';
    form = { ...form, matchDimension, matchValue };
  }

  async function saveRule(): Promise<void> {
    if (saving) return;
    error = '';
    status = '';
    if (!form.matchValue) {
      error = '请选择这条规则适用的范围。';
      return;
    }
    if (!/^\S+@\S+\.\S+$/.test(form.recipient.trim())) {
      error = '请输入有效的收件邮箱。';
      return;
    }
    if (form.enabled && !smtpStatus.verifiedAt) {
      error = '连接尚未通过测试。规则可以先保存为停用，测试成功后再启用。';
      return;
    }
    saving = true;
    try {
      const saved = await api.saveEmailDeliveryRule({ ...form, recipient: form.recipient.trim() });
      rules = form.id ? rules.map((rule) => rule.id === saved.id ? saved : rule) : [...rules, saved];
      editing = false;
      status = '邮件提醒规则已保存。';
    } catch (cause) {
      error = readableError(cause, '规则没有保存，你的输入仍在，可以重试。');
    } finally {
      saving = false;
    }
  }

  async function removeRule(rule: EmailDeliveryRule): Promise<void> {
    if (!confirm(`删除“${ruleLabel(rule)}”邮件规则？已经投递的邮件不会受影响。`)) return;
    error = '';
    try {
      await api.deleteEmailDeliveryRule(rule.id);
      rules = rules.filter((value) => value.id !== rule.id);
      status = '邮件提醒规则已删除。';
    } catch (cause) {
      error = readableError(cause, '规则没有删除，请重试。');
    }
  }

  function readableError(_cause: unknown, fallback: string): string {
    return fallback;
  }

  function matchLabel(rule: Pick<EmailDeliveryRule, 'matchDimension' | 'matchValue'>): string {
    if (rule.matchDimension === 'EVENT_KIND') return eventKindLabel(rule.matchValue as Parameters<typeof eventKindLabel>[0]);
    if (rule.matchDimension === 'CATEGORY') return categories.find((value) => value.id === rule.matchValue)?.name ?? '已删除分类';
    return tags.find((value) => value.id === rule.matchValue)?.name ?? '已删除标签';
  }

  function ruleLabel(rule: EmailDeliveryRule): string {
    return `${dimensionOptions.find((value) => value.value === rule.matchDimension)?.label}“${matchLabel(rule)}”`;
  }

  function strategyLabel(value: EmailDeliveryStrategy): string {
    return strategyOptions.find((option) => option.value === value)?.label ?? value;
  }

  function connectionTime(value: string | null): string {
    if (!value) return '尚未通过发送测试';
    return `最近验证于 ${new Intl.DateTimeFormat('zh-CN', { month: 'numeric', day: 'numeric', hour: '2-digit', minute: '2-digit' }).format(new Date(value))}`;
  }
</script>

<div class="panel-backdrop" role="presentation" on:click={(event) => event.currentTarget === event.target && onClose()}>
  <div class="side-panel email-rules-panel" role="dialog" aria-modal="true" aria-labelledby="email-rules-title">
    <header class="panel-header">
      <div>
        <p class="eyebrow">邮件工作台</p>
        <h2 id="email-rules-title">邮件提醒</h2>
        <p>决定哪些事项、在什么时机、发送到哪里。</p>
      </div>
      <button class="icon-button" aria-label="关闭邮件提醒" on:click={onClose}>×</button>
    </header>

    <div class:verified={Boolean(smtpStatus.verifiedAt)} class="email-connection-strip">
      <span class="connection-dot" aria-hidden="true"></span>
      <div><strong>{smtpStatus.verifiedAt ? '发送连接可用' : '发送连接待验证'}</strong><small>{connectionTime(smtpStatus.verifiedAt)}</small></div>
      <button class="text-mini" on:click={onOpenSettings}>打开连接设置</button>
    </div>

    <div class="delivery-rail" aria-label="邮件投递路径">
      <span>事项命中</span><i aria-hidden="true"></i><span>投递时机</span><i aria-hidden="true"></i><span>你的邮箱</span>
    </div>

    {#if error}<div class="inline-error compact" role="alert"><span>{error}</span></div>{/if}
    {#if status}<p class="panel-status" role="status">{status}</p>{/if}

    {#if editing}
      <form class="email-rule-editor" on:submit|preventDefault={saveRule}>
        <div class="rule-editor-heading"><strong>{form.id ? '编辑投递规则' : '新建投递规则'}</strong><small>重叠规则发送到同一邮箱时只投递一次。</small></div>
        <div class="rule-condition-grid">
          <label><span>按什么匹配</span><select value={form.matchDimension} on:change={(event) => setDimension(event.currentTarget.value)}><option value="EVENT_KIND">事件类型</option><option value="CATEGORY">分类</option><option value="TAG">标签</option></select></label>
          <label><span>匹配内容</span><select bind:value={form.matchValue} disabled={matchOptions.length === 0}>{#each matchOptions as option}<option value={option.value}>{option.label}</option>{/each}</select></label>
        </div>
        {#if matchOptions.length === 0}<p class="field-help">当前没有可选内容，请先在“整理方式”中添加。</p>{/if}
        <label class="form-field"><span>何时发送</span><select bind:value={form.strategy}>{#each strategyOptions as option}<option value={option.value}>{option.label} · {option.help}</option>{/each}</select></label>
        {#if form.strategy === 'DAILY_DIGEST'}
          <label class="form-field narrow-field"><span>每日汇总时间</span><input type="time" bind:value={form.digestLocalTime} required /></label>
        {/if}
        <label class="form-field"><span>收件邮箱</span><input type="email" bind:value={form.recipient} placeholder="me@example.com" autocomplete="email" required /></label>
        <label class="switch-row compact-switch"><span><strong>启用这条规则</strong><small>{smtpStatus.verifiedAt ? '保存后开始按规则投递' : '需要先通过发送测试'}</small></span><input class="switch" type="checkbox" bind:checked={form.enabled} disabled={!smtpStatus.verifiedAt} /></label>
        <div class="panel-actions"><button type="button" class="secondary-button" on:click={() => (editing = false)}>取消</button><button class="primary-button" disabled={saving || matchOptions.length === 0}>{saving ? '正在保存…' : '保存规则'}</button></div>
      </form>
    {:else}
      <div class="email-rule-toolbar"><div><strong>{rules.length} 条投递规则</strong><small>停用规则仍会保留，之后可重新启用。</small></div><button class="primary-button" on:click={beginCreate}>新建规则</button></div>
    {/if}

    {#if loading}
      <div class="panel-loading" aria-label="正在读取邮件规则"><span></span><span></span><span></span></div>
    {:else if rules.length === 0 && !editing}
      <div class="panel-empty"><span class="empty-envelope" aria-hidden="true">↗</span><strong>还没有邮件提醒规则</strong><p>新建一条规则后，只有命中的事项会进入邮件投递路径。</p><button class="secondary-button" on:click={beginCreate}>建立第一条规则</button></div>
    {:else}
      <div class="email-rule-list">
        {#each rules as rule (rule.id)}
          <article class:disabled={!rule.enabled} class="email-rule-card">
            <span class="rule-state" aria-hidden="true"></span>
            <div class="rule-copy"><p><strong>{ruleLabel(rule)}</strong><span>→</span><strong>{strategyLabel(rule.strategy)}</strong></p><small>{rule.recipient}{rule.strategy === 'DAILY_DIGEST' ? ` · ${rule.digestLocalTime} 汇总` : ''}</small></div>
            <span class="state-label">{rule.enabled ? '已启用' : '已停用'}</span>
            <button class="text-mini" on:click={() => beginEdit(rule)}>编辑</button>
            <button class="text-mini danger-text" on:click={() => removeRule(rule)}>删除</button>
          </article>
        {/each}
      </div>
    {/if}
  </div>
</div>
