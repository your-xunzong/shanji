import { fireEvent, render, screen } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';
import DataRecoveryView from './DataRecoveryView.svelte';
import type { StartupStatus } from '../lib/types';

const recoveryStatus: StartupStatus = {
  mode: 'recovery_required',
  targetPath: 'C:\\Users\\test\\AppData\\Roaming\\com.shanji.desktop\\shanji.db',
  message: '当前数据位置没有记录，但找到了以前保存的内容。恢复前不会写入空数据。',
  current: null,
  recoveryCandidates: [{
    path: 'D:\\old-shanji\\data\\shanji.db',
    itemCount: 12,
    reminderHistoryCount: 7,
    hasSettings: true,
    hasDraft: true,
    hasCustomSettings: true,
    schemaVersion: 7,
    updatedAt: '2026-09-01T10:20:00Z',
  }],
  diagnostic: 'startup_state=legacy_data_found; candidates=1; app_schema=7',
};

describe('DataRecoveryView', () => {
  it('只展示经过校验的恢复任务并在确认后恢复', async () => {
    const onRestore = vi.fn().mockResolvedValue(undefined);
    vi.spyOn(window, 'confirm').mockReturnValue(true);
    render(DataRecoveryView, {
      status: recoveryStatus,
      onRestore,
      onOpenLocation: vi.fn().mockResolvedValue(undefined),
      onRetry: vi.fn().mockResolvedValue(undefined),
    });

    expect(screen.getByRole('heading', { name: '找到原来的数据' })).toBeInTheDocument();
    expect(screen.getByText('12 项记录')).toBeInTheDocument();
    expect(screen.getByText('7 条提醒历史')).toBeInTheDocument();
    expect(screen.getByText('个性设置已找到')).toBeInTheDocument();
    expect(screen.getByText('未提交草稿已找到')).toBeInTheDocument();
    expect(screen.queryByText(/数据结构/)).not.toBeInTheDocument();

    await fireEvent.click(screen.getByRole('button', { name: '恢复原数据并重新启动' }));
    expect(onRestore).toHaveBeenCalledWith(recoveryStatus.recoveryCandidates[0].path);
  });

  it('没有安全候选时停止写入并保留重试入口', async () => {
    const onRetry = vi.fn().mockRejectedValue(new Error('磁盘空间仍然不足，原数据未修改。'));
    render(DataRecoveryView, {
      status: {
        ...recoveryStatus,
        mode: 'blocked',
        recoveryCandidates: [],
        message: '原数据仍在，但可用空间不足，无法安全完成备份或升级。释放空间后可以重试。',
      },
      onRestore: vi.fn().mockResolvedValue(undefined),
      onOpenLocation: vi.fn().mockResolvedValue(undefined),
      onRetry,
    });

    expect(screen.getByRole('heading', { name: '原数据仍在' })).toBeInTheDocument();
    expect(screen.getByRole('alert')).toHaveTextContent('没有创建新数据');
    expect(screen.queryByRole('button', { name: '恢复原数据并重新启动' })).not.toBeInTheDocument();
    await fireEvent.click(screen.getByRole('button', { name: '重新检查' }));
    expect(await screen.findByText('磁盘空间仍然不足，原数据未修改。')).toBeInTheDocument();
  });
});
