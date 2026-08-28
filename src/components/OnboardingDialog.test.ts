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

function setup(firstRun = true) {
  const onFinish = vi.fn().mockResolvedValue(undefined);
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
    const input = screen.getByRole('textbox', { name: '全局快捷键' });
    await fireEvent.input(input, { target: { value: 'Ctrl+Shift' } });
    expect(screen.getByRole('alert')).toHaveTextContent('快捷键必须包含一个普通按键');
    expect(screen.getByRole('button', { name: '完成设置' })).toBeDisabled();
  });
});
