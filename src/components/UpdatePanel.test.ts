import { fireEvent, render, screen } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';
import UpdatePanel from './UpdatePanel.svelte';

const callbacks = {
  onClose: vi.fn(), onRetry: vi.fn(), onDownload: vi.fn(), onCancelDownload: vi.fn(),
  onInstall: vi.fn(), onSnooze: vi.fn(), onOpenRelease: vi.fn(),
};

const update = {
  currentVersion: '0.12.0',
  version: '0.12.1',
  date: '2026-09-09T08:00:00Z',
  body: '- 修复提醒',
  notes: ['修复提醒', '改进字体'],
};

describe('UpdatePanel', () => {
  it('shows release notes before downloading', async () => {
    render(UpdatePanel, {
      phase: 'available', update, currentVersion: '0.12.0', currentNotes: [], ...callbacks,
    });

    expect(screen.getByRole('heading', { name: '更新到 v0.12.1' })).toBeInTheDocument();
    expect(screen.getByText('修复提醒')).toBeInTheDocument();
    await fireEvent.click(screen.getByRole('button', { name: '下载更新' }));
    expect(callbacks.onDownload).toHaveBeenCalled();
  });

  it('uses a manual download action for unsupported package modes', () => {
    render(UpdatePanel, {
      phase: 'available', update, currentVersion: '0.12.0', currentNotes: [], installMode: 'DOWNLOAD_ONLY', ...callbacks,
    });

    expect(screen.getByRole('button', { name: '打开官方下载页' })).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: '下载更新' })).not.toBeInTheDocument();
  });

  it('explains that cancelling preserves the current version', () => {
    render(UpdatePanel, {
      phase: 'cancelled', update, currentVersion: '0.12.0', currentNotes: [], ...callbacks,
    });

    expect(screen.getByText('下载已取消，当前版本和本机数据没有改变。')).toBeInTheDocument();
  });

  it('shows the installed release as one current-version node', () => {
    const { container } = render(UpdatePanel, {
      phase: 'current', update: null, currentVersion: '0.12.0', currentNotes: ['本版说明'], ...callbacks,
    });

    expect(screen.getByRole('heading', { name: '本版更新说明' })).toBeInTheDocument();
    expect(screen.getAllByText('v0.12.0')).toHaveLength(1);
    expect(container.querySelector('.current-release-version')).toBeInTheDocument();
    expect(container.querySelector('.version-track')).not.toBeInTheDocument();
  });

  it('does not close from Escape while an update is downloading', async () => {
    callbacks.onClose.mockClear();
    render(UpdatePanel, {
      phase: 'downloading', update, currentVersion: '0.12.0', currentNotes: [], ...callbacks,
    });

    await fireEvent.keyDown(window, { key: 'Escape' });
    expect(callbacks.onClose).not.toHaveBeenCalled();
  });
});
