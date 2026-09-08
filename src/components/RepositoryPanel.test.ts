import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { open } from '@tauri-apps/plugin-dialog';
import RepositoryPanel from './RepositoryPanel.svelte';
import { api } from '../lib/api';

vi.mock('@tauri-apps/plugin-dialog', () => ({ open: vi.fn() }));

afterEach(() => {
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

describe('RepositoryPanel', () => {
  describe('A1 原生仓库选择', () => {
    const empty = { configured: false, path: null, available: false, localItemCount: 18, packageCount: 0, pendingSourceCount: 0, conflictCount: 0, lastSyncedAt: null, lastPackageAt: null, snapshotReady: false, lastErrorCode: null, message: '' };
    const configured = { ...empty, configured: true, available: true, path: 'D:\\资料\\shanji-repository', packageCount: 1, lastPackageAt: '2026-09-08T08:00:00Z', snapshotReady: true };

    beforeEach(() => {
      vi.stubGlobal('isTauri', true);
      vi.spyOn(api, 'getRepositoryStatus').mockResolvedValue(empty);
      vi.spyOn(api, 'configureDataRepository').mockResolvedValue(configured);
      vi.spyOn(window, 'confirm').mockReturnValue(true);
      vi.mocked(open).mockReset().mockResolvedValue('D:\\资料');
    });

    it('选择中文目录并确认后显示仓库路径，保留本机记录数', async () => {
      render(RepositoryPanel, { onClose: vi.fn() });
      await fireEvent.click(await screen.findByRole('button', { name: '选择仓库位置' }));
      expect(await screen.findByText(configured.path)).toBeInTheDocument();
      expect(open).toHaveBeenCalledWith({ directory: true, multiple: false, title: '选择个人仓库所在文件夹' });
      expect(api.configureDataRepository).toHaveBeenCalledWith('D:\\资料');
      expect(screen.getByText('18 条记录')).toBeInTheDocument();
      expect(screen.getByText('个人仓库已建立，已写入 18 条本机记录。')).toBeInTheDocument();
    });

    it('取消选择不弹确认、不写配置', async () => {
      vi.mocked(open).mockResolvedValue(null);
      render(RepositoryPanel, { onClose: vi.fn() });
      await fireEvent.click(await screen.findByRole('button', { name: '选择仓库位置' }));
      await waitFor(() => expect(screen.getByRole('button', { name: '选择仓库位置' })).toBeEnabled());
      expect(window.confirm).not.toHaveBeenCalled();
      expect(api.configureDataRepository).not.toHaveBeenCalled();
      expect(screen.queryByRole('alert')).not.toBeInTheDocument();
    });

    it('取消确认不写配置', async () => {
      vi.mocked(window.confirm).mockReturnValue(false);
      render(RepositoryPanel, { onClose: vi.fn() });
      await fireEvent.click(await screen.findByRole('button', { name: '选择仓库位置' }));
      await waitFor(() => expect(window.confirm).toHaveBeenCalled());
      expect(api.configureDataRepository).not.toHaveBeenCalled();
    });

    it('原生选择器失败可见且重试会重新打开，不泄露权限错误', async () => {
      vi.mocked(open).mockRejectedValueOnce(new Error('dialog.open not allowed: missing dialog:allow-open'));
      render(RepositoryPanel, { onClose: vi.fn() });
      await fireEvent.click(await screen.findByRole('button', { name: '选择仓库位置' }));
      expect(await screen.findByRole('alert')).toHaveTextContent('文件夹选择窗口未能打开。本机记录和仓库设置没有改变，请重试。');
      expect(screen.queryByText(/dialog:allow-open/)).not.toBeInTheDocument();
      expect(api.configureDataRepository).not.toHaveBeenCalled();
      await fireEvent.click(screen.getByRole('button', { name: '重新选择文件夹' }));
      expect(await screen.findByText(configured.path)).toBeInTheDocument();
      expect(open).toHaveBeenCalledTimes(2);
    });

    it('保存失败不显示配置成功，允许重新选择', async () => {
      vi.mocked(api.configureDataRepository).mockRejectedValueOnce(new Error('os error 5'));
      render(RepositoryPanel, { onClose: vi.fn() });
      await fireEvent.click(await screen.findByRole('button', { name: '选择仓库位置' }));
      expect(await screen.findByRole('alert')).toHaveTextContent('个人仓库没有建立。本机记录仍保存在原位置，可以换一个文件夹重试。');
      expect(screen.queryByText('仓库可用')).not.toBeInTheDocument();
      await fireEvent.click(screen.getByRole('button', { name: '重新选择文件夹' }));
      expect(await screen.findByText(configured.path)).toBeInTheDocument();
    });

    it('选择器未关闭时不会重复打开', async () => {
      let finish!: (value: null) => void;
      vi.mocked(open).mockReturnValueOnce(new Promise((resolve) => { finish = resolve; }));
      render(RepositoryPanel, { onClose: vi.fn() });
      const button = await screen.findByRole('button', { name: '选择仓库位置' });
      await fireEvent.click(button);
      await waitFor(() => expect(open).toHaveBeenCalledTimes(1));
      expect(button).toBeDisabled();
      await fireEvent.click(button);
      expect(open).toHaveBeenCalledTimes(1);
      finish(null);
      await waitFor(() => expect(button).toBeEnabled());
    });

    it('首次快照未写入时保留仓库位置并可重新写入', async () => {
      const pending = { ...configured, packageCount: 0, lastPackageAt: null, snapshotReady: false, lastErrorCode: 'snapshot_write_failed' };
      vi.mocked(api.configureDataRepository).mockResolvedValueOnce(pending);
      const publish = vi.spyOn(api, 'publishDataRepositorySnapshot').mockResolvedValue(configured);
      render(RepositoryPanel, { onClose: vi.fn() });

      await fireEvent.click(await screen.findByRole('button', { name: '选择仓库位置' }));
      expect(await screen.findByRole('alert')).toHaveTextContent('本机快照尚未写入');
      await fireEvent.click(screen.getByRole('button', { name: '重新写入' }));
      expect(publish).toHaveBeenCalledTimes(1);
      expect(await screen.findByText('已向个人仓库写入 18 条本机记录。')).toBeInTheDocument();
    });
  });

  it('仓库暂时不可用时明确说明本机记录仍可使用', async () => {
    vi.spyOn(api, 'getRepositoryStatus').mockResolvedValue({
      configured: true,
      path: 'Z:\\shanji-repository',
      available: false,
      localItemCount: 18,
      packageCount: 0,
      pendingSourceCount: 0,
      conflictCount: 0,
      lastSyncedAt: null,
      lastPackageAt: null,
      snapshotReady: false,
      lastErrorCode: 'repository_unavailable',
      message: '仓库暂时不可用',
    });
    render(RepositoryPanel, { onClose: vi.fn() });

    expect(await screen.findByText('仓库暂时不可用')).toBeInTheDocument();
    expect(screen.getByText('本机记录照常保存；重新连接此位置后再同步。')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: '检查其他设备的数据' })).toBeDisabled();
  });

  it('同步前展示冲突影响并要求二次确认', async () => {
    vi.spyOn(api, 'getRepositoryStatus').mockResolvedValue({ configured: true, path: 'D:\\repo', available: true, localItemCount: 10, packageCount: 2, pendingSourceCount: 1, conflictCount: 1, lastSyncedAt: null, lastPackageAt: '2026-09-03T07:00:00Z', snapshotReady: true, lastErrorCode: null, message: '' });
    vi.spyOn(api, 'previewRepositoryMerge').mockResolvedValue({
      sources: [{ sourceInstanceId: 'device-2', exportedAt: '2026-09-03T08:00:00Z', itemCount: 3, newItems: 1, unchangedItems: 1, conflicts: 1, tombstones: 0 }],
      newItems: 1,
      unchangedItems: 1,
      conflicts: 1,
      tombstones: 0,
      settingsNeedReview: false,
    });
    const sync = vi.spyOn(api, 'syncDataRepository').mockResolvedValue({ importedItems: 1, unchangedItems: 1, conflicts: 1, tombstonesApplied: 0, packagePath: 'D:\\repo\\a.sjpack', packageWritten: true, syncedAt: '2026-09-03T08:00:00Z' });
    vi.spyOn(window, 'confirm').mockReturnValue(false);
    render(RepositoryPanel, { onClose: vi.fn() });

    await fireEvent.click(await screen.findByRole('button', { name: '检查其他设备的数据' }));
    expect(await screen.findByText(/冲突不会被覆盖/)).toBeInTheDocument();
    await fireEvent.click(screen.getByRole('button', { name: '汇入并更新仓库' }));
    expect(window.confirm).toHaveBeenCalledWith(expect.stringContaining('冲突副本'));
    expect(sync).not.toHaveBeenCalled();
  });

  it('汇入成功后刷新完整数据并可查看全部事项', async () => {
    const status = { configured: true, path: 'D:\\repo', available: true, localItemCount: 12, packageCount: 2, pendingSourceCount: 1, conflictCount: 0, lastSyncedAt: '2026-09-08T08:00:00Z', lastPackageAt: '2026-09-08T08:00:00Z', snapshotReady: true, lastErrorCode: null, message: '' };
    vi.spyOn(api, 'getRepositoryStatus').mockResolvedValue(status);
    vi.spyOn(api, 'previewRepositoryMerge').mockResolvedValue({
      sources: [{ sourceInstanceId: 'device-2', exportedAt: '2026-09-08T08:00:00Z', itemCount: 2, newItems: 2, unchangedItems: 0, conflicts: 0, tombstones: 0 }],
      newItems: 2, unchangedItems: 0, conflicts: 0, tombstones: 0, settingsNeedReview: false,
    });
    vi.spyOn(api, 'syncDataRepository').mockResolvedValue({ importedItems: 2, unchangedItems: 0, conflicts: 0, tombstonesApplied: 0, packagePath: 'D:\\repo\\device.sjpack', packageWritten: true, syncedAt: '2026-09-08T08:00:00Z' });
    vi.spyOn(window, 'confirm').mockReturnValue(true);
    const changed = vi.fn().mockResolvedValue(undefined);
    const showAll = vi.fn().mockResolvedValue(undefined);
    render(RepositoryPanel, { onClose: vi.fn(), onItemsChanged: changed, onShowAllItems: showAll });

    await fireEvent.click(await screen.findByRole('button', { name: '检查其他设备的数据' }));
    await fireEvent.click(await screen.findByRole('button', { name: '汇入并更新仓库' }));
    expect(await screen.findByText(/已汇入 2 条记录/)).toBeInTheDocument();
    expect(changed).toHaveBeenCalledTimes(1);
    await fireEvent.click(screen.getByRole('button', { name: '查看全部事项' }));
    expect(showAll).toHaveBeenCalledTimes(1);
  });

  it('状态读取失败时只显示可恢复文案', async () => {
    vi.spyOn(api, 'getRepositoryStatus').mockRejectedValue(new Error('SELECT failed at repository_packages'));
    render(RepositoryPanel, { onClose: vi.fn() });

    expect(await screen.findByText('个人仓库状态暂时无法读取。本机记录不受影响。')).toBeInTheDocument();
    expect(screen.queryByText(/repository_packages/)).not.toBeInTheDocument();
  });
});
