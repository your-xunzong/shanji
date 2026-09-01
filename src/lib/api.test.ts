import { beforeEach, describe, expect, it, vi } from 'vitest';

const { invokeMock } = vi.hoisted(() => ({ invokeMock: vi.fn() }));

vi.mock('@tauri-apps/api/core', () => ({
  invoke: invokeMock,
  isTauri: () => true,
}));

import { api } from './api';
import type { Settings } from './types';

const settings: Settings = {
  defaultDueTime: '18:00',
  repeatDefaultTimes: ['10:00', '17:00'],
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

describe('Tauri startup calls', () => {
  beforeEach(() => invokeMock.mockReset());

  it('retries when managed state is not ready yet', async () => {
    invokeMock
      .mockRejectedValueOnce(
        'state not managed for field `state` on command `get_settings`. You must call `.manage()` before using this command',
      )
      .mockResolvedValueOnce(settings);

    await expect(api.getSettings()).resolves.toEqual(settings);
    expect(invokeMock).toHaveBeenCalledTimes(2);
  });

  it('does not retry ordinary command errors', async () => {
    invokeMock.mockRejectedValueOnce('数据库只读');

    await expect(api.getSettings()).rejects.toThrow('数据库只读');
    expect(invokeMock).toHaveBeenCalledTimes(1);
  });
});
