import { fireEvent, render, screen } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';
import OnboardingDialog from './OnboardingDialog.svelte';
import type { AutostartStatus, NotificationStatus, Settings } from '../lib/types';

const settings: Settings = {
  defaultDueTime: '18:00',
  workdays: [1, 2, 3, 4, 5],
  overtimeIntervalMinutes: 30,
  quietHoursEnabled: true,
  quietStart: '22:30',
  quietEnd: '07:30',
  globalShortcut: 'CommandOrControl+Shift+Space',
  notificationsEnabled: true,
  autostartEnabled: false,
  persistentNotificationsEnabled: true,
  overlayRemindersEnabled: false,
  repeatUnacknowledgedEnabled: false,
  unacknowledgedRepeatMinutes: 60,
  smtpEnabled: false,
  smtpHost: '',
  smtpPort: 465,
  smtpSecurity: 'tls',
  smtpFrom: '',
  smtpTo: '',
  smtpUsername: '',
  smtpRepeatMustComplete: false,
  eventKindDefaults: [
    { eventKind: 'ORDINARY', reminderPlan: 'REPEAT' },
    { eventKind: 'ONE_TIME', reminderPlan: 'ONCE' },
    { eventKind: 'TODAY_MUST', reminderPlan: 'EMPHASIS' },
    { eventKind: 'WARNING', reminderPlan: 'REPEAT' },
    { eventKind: 'CONTINUOUS', reminderPlan: 'CUSTOM' },
    { eventKind: 'MONTHLY', reminderPlan: 'ONCE' },
    { eventKind: 'YEARLY', reminderPlan: 'ONCE' },
  ],
};

const notificationStatus: NotificationStatus = {
  platform: 'windows',
  portable: false,
  identityStatus: 'ready',
  canNotify: true,
  message: '系统通知身份已就绪。',
  lastResult: null,
  lastErrorCode: null,
  lastAttemptAt: null,
};

const autostartStatus: AutostartStatus = {
  enabled: false,
  available: true,
  portable: false,
  message: '开机启动当前未启用。',
};

function setup(firstRun = true, finishError?: Error) {
  const onFinish = finishError
    ? vi.fn().mockRejectedValue(finishError)
    : vi.fn().mockResolvedValue(undefined);
  const onSkip = vi.fn().mockResolvedValue(undefined);
  render(OnboardingDialog, {
    settings,
    notificationStatus,
    autostartStatus,
    firstRun,
    onFinish,
    onSkip,
  });
  return { onFinish, onSkip };
}

describe('OnboardingDialog', () => {
  it('首次引导默认勾选开机启动，但继续前不执行保存', async () => {
    const { onFinish } = setup(true);
    await fireEvent.click(screen.getByRole('button', { name: '继续' }));
    expect(screen.getByRole('checkbox', { name: /登录后自动启动闪记/ })).toBeChecked();
    expect(onFinish).not.toHaveBeenCalled();
  });

  it('跳过引导不会提交任何系统选项', async () => {
    const { onFinish, onSkip } = setup(true);
    await fireEvent.click(screen.getByRole('button', { name: '暂时跳过' }));
    expect(onSkip).toHaveBeenCalledOnce();
    expect(onFinish).not.toHaveBeenCalled();
  });

  it('只有修饰键时显示具体错误并禁止完成', async () => {
    setup(true);
    await fireEvent.click(screen.getByRole('button', { name: '继续' }));
    await fireEvent.click(screen.getByRole('button', { name: '继续' }));
    await fireEvent.click(screen.getByRole('button', { name: '继续' }));
    const input = screen.getByRole('textbox', { name: '全局快捷键' });
    await fireEvent.input(input, { target: { value: 'Ctrl+Shift' } });
    expect(screen.getByRole('alert')).toHaveTextContent('快捷键必须包含一个普通按键');
    expect(screen.getByRole('button', { name: '完成设置' })).toBeDisabled();
  });

  it('显示当前默认时间并允许使用常用时间完成设置', async () => {
    const { onFinish } = setup(true);
    await fireEvent.click(screen.getByRole('button', { name: '继续' }));
    await fireEvent.click(screen.getByRole('button', { name: '继续' }));

    const timeInput = screen.getByLabelText('默认提醒时间');
    expect(timeInput).toHaveValue('18:00');
    await fireEvent.click(screen.getByRole('button', { name: '17:30' }));
    expect(timeInput).toHaveValue('17:30');

    await fireEvent.click(screen.getByRole('button', { name: '继续' }));
    await fireEvent.click(screen.getByRole('button', { name: '完成设置' }));
    expect(onFinish).toHaveBeenCalledWith(expect.objectContaining({ defaultDueTime: '17:30' }));
  });

  it('默认提醒时间为空时不能继续', async () => {
    setup(true);
    await fireEvent.click(screen.getByRole('button', { name: '继续' }));
    await fireEvent.click(screen.getByRole('button', { name: '继续' }));

    await fireEvent.input(screen.getByLabelText('默认提醒时间'), { target: { value: '' } });
    expect(screen.getByRole('alert')).toHaveTextContent('请选择默认提醒时间');
    expect(screen.getByRole('button', { name: '继续' })).toBeDisabled();
  });

  it('保存失败时保留用户选择并显示友好提示', async () => {
    setup(true, new Error('默认提醒时间没有保存，原设置仍然有效，请重试。'));
    await fireEvent.click(screen.getByRole('button', { name: '继续' }));
    await fireEvent.click(screen.getByRole('button', { name: '继续' }));
    await fireEvent.click(screen.getByRole('button', { name: '18:30' }));
    await fireEvent.click(screen.getByRole('button', { name: '继续' }));
    await fireEvent.click(screen.getByRole('button', { name: '完成设置' }));

    expect(await screen.findByRole('alert')).toHaveTextContent('原设置仍然有效');
    await fireEvent.click(screen.getByRole('button', { name: '上一步' }));
    expect(screen.getByLabelText('默认提醒时间')).toHaveValue('18:30');
  });
});
