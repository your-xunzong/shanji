import { fireEvent, render, screen } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';
import SettingsPanel from './SettingsPanel.svelte';
import type { AutostartStatus, NotificationStatus, Settings } from '../lib/types';

const settings: Settings = {
  defaultDueTime: '18:00',
  workdays: [1, 2, 3, 4, 5],
  overtimeIntervalMinutes: 30,
  quietHoursEnabled: true,
  quietStart: '22:30',
  quietEnd: '07:30',
  globalShortcut: 'Ctrl+Shift+Space',
  notificationsEnabled: true,
  autostartEnabled: false,
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
      onRegisterNotifications: vi.fn().mockResolvedValue(undefined),
      onUnregisterNotifications: vi.fn().mockResolvedValue(undefined),
      onOpenOnboarding: vi.fn(),
    });

    await fireEvent.click(screen.getByRole('button', { name: '保存设置' }));
    expect(await screen.findByRole('alert')).toHaveTextContent(message);
  });
});
