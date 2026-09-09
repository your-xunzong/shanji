import { fireEvent, render, screen } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';
import SettingsPanel from './SettingsPanel.svelte';
import type { AutostartStatus, NotificationStatus, Settings } from '../lib/types';

const settings: Settings = {
  defaultDueTime: '18:00',
  repeatDefaultTimes: ['10:00', '17:00'],
  workdays: [1, 2, 3, 4, 5],
  overtimeIntervalMinutes: 30,
  quietHoursEnabled: true,
  quietStart: '22:30',
  quietEnd: '07:30',
  globalShortcut: 'Ctrl+Shift+Space',
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
  message: '原生通知已就绪。',
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

describe('SettingsPanel', () => {
  it('separates reminder display from schedule and opens the real reminder center', async () => {
    const onSave = vi.fn().mockResolvedValue(undefined);
    const onOpenReminderCenter = vi.fn();
    render(SettingsPanel, {
      settings: { ...settings, repeatUnacknowledgedEnabled: true },
      saving: false,
      notificationStatus,
      autostartStatus,
      pendingReminderCount: 3,
      onClose: vi.fn(),
      onSave,
      onTestNotification: vi.fn().mockResolvedValue(undefined),
      onTestReminderMode: vi.fn().mockResolvedValue('测试已发送'),
      onOpenReminderCenter,
      onRegisterNotifications: vi.fn().mockResolvedValue(undefined),
      onUnregisterNotifications: vi.fn().mockResolvedValue(undefined),
      onOpenOnboarding: vi.fn(),
    });

    expect(screen.getByText('提醒时间与频率由事件默认方案和单条事项决定；这里仅选择提醒从哪里出现。')).toBeInTheDocument();
    expect(screen.getByText('系统')).toBeInTheDocument();
    expect(screen.getAllByText('闪记').length).toBeGreaterThan(0);
    expect(screen.getByText('3 条')).toBeInTheDocument();
    expect(screen.queryByText('普通事项未确认时重复提醒')).not.toBeInTheDocument();
    expect(screen.queryByRole('combobox', { name: '普通事项重复提醒间隔' })).not.toBeInTheDocument();

    await fireEvent.click(screen.getByRole('button', { name: '打开待确认提醒' }));
    expect(onOpenReminderCenter).toHaveBeenCalledOnce();
    await fireEvent.click(screen.getByRole('button', { name: '保存设置' }));
    expect(onSave).toHaveBeenCalledWith(expect.objectContaining({
      repeatUnacknowledgedEnabled: false,
    }));
  });

  it('显示运行时版本并保存事件默认方案', async () => {
    const onSave = vi.fn().mockResolvedValue(undefined);
    render(SettingsPanel, {
      settings,
      saving: false,
      notificationStatus,
      autostartStatus,
      appInfo: { name: '闪记', version: '0.7.0', copyright: '© 2026 闪记', portable: false, updateInstallMode: 'AUTOMATIC' },
      onClose: vi.fn(),
      onSave,
      onTestNotification: vi.fn().mockResolvedValue(undefined),
      onTestReminderMode: vi.fn().mockResolvedValue(undefined),
      onRegisterNotifications: vi.fn().mockResolvedValue(undefined),
      onUnregisterNotifications: vi.fn().mockResolvedValue(undefined),
      onOpenOnboarding: vi.fn(),
    });

    expect(screen.getByText('版本 0.7.0')).toBeInTheDocument();
    await fireEvent.change(screen.getByLabelText(/^今日必做/), { target: { value: 'FORCE' } });
    await fireEvent.click(screen.getByRole('button', { name: '保存设置' }));
    expect(onSave).toHaveBeenCalledWith(expect.objectContaining({
      eventKindDefaults: expect.arrayContaining([
        { eventKind: 'TODAY_MUST', reminderPlan: 'FORCE' },
      ]),
    }));
  });

  it('原样展示后端返回的快捷键错误', async () => {
    const message = '快捷键“Ctrl+Shift+Space”已被其他程序占用；原快捷键仍然有效。';
    render(SettingsPanel, {
      settings,
      saving: false,
      notificationStatus,
      autostartStatus,
      onClose: vi.fn(),
      onSave: vi.fn().mockRejectedValue(new Error(message)),
      onTestNotification: vi.fn().mockResolvedValue(undefined),
      onTestReminderMode: vi.fn().mockResolvedValue(undefined),
      onRegisterNotifications: vi.fn().mockResolvedValue(undefined),
      onUnregisterNotifications: vi.fn().mockResolvedValue(undefined),
      onOpenOnboarding: vi.fn(),
    });

    await fireEvent.click(screen.getByRole('button', { name: '保存设置' }));
    expect(await screen.findByRole('alert')).toHaveTextContent(message);
  });

  it('重复提醒默认时间相同时不提交设置', async () => {
    const onSave = vi.fn().mockResolvedValue(undefined);
    render(SettingsPanel, {
      settings,
      saving: false,
      notificationStatus,
      autostartStatus,
      onClose: vi.fn(),
      onSave,
      onTestNotification: vi.fn().mockResolvedValue(undefined),
      onTestReminderMode: vi.fn().mockResolvedValue(undefined),
      onRegisterNotifications: vi.fn().mockResolvedValue(undefined),
      onUnregisterNotifications: vi.fn().mockResolvedValue(undefined),
      onOpenOnboarding: vi.fn(),
    });
    await fireEvent.input(screen.getByLabelText('第二次'), { target: { value: '10:00' } });
    expect(screen.getByRole('alert')).toHaveTextContent('不能使用相同时间');
    expect(screen.getByRole('button', { name: '保存设置' })).toBeDisabled();
    expect(onSave).not.toHaveBeenCalled();
  });

  it('submits an SMTP password separately from ordinary settings', async () => {
    const onSave = vi.fn().mockResolvedValue(undefined);
    render(SettingsPanel, {
      settings: {
        ...settings,
        smtpEnabled: true,
        smtpHost: 'smtp.example.com',
        smtpFrom: 'me@example.com',
        smtpTo: 'me@example.com',
      },
      saving: false,
      notificationStatus,
      autostartStatus,
      onClose: vi.fn(),
      onSave,
      onTestNotification: vi.fn().mockResolvedValue(undefined),
      onTestReminderMode: vi.fn().mockResolvedValue('已准备'),
      onRegisterNotifications: vi.fn().mockResolvedValue(undefined),
      onUnregisterNotifications: vi.fn().mockResolvedValue(undefined),
      onOpenOnboarding: vi.fn(),
    });

    await fireEvent.input(screen.getByLabelText('SMTP 密码'), { target: { value: 'app-secret' } });
    await fireEvent.click(screen.getByRole('button', { name: '保存设置' }));
    expect(onSave).toHaveBeenCalledWith(expect.objectContaining({ smtpPassword: 'app-secret' }));
  });
});
