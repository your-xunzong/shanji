import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import TimelineView from './TimelineView.svelte';
import { api } from '../lib/api';
import type { TimelineData, TimelineEntry } from '../lib/types';

function entry(id: string, overrides: Partial<TimelineEntry> = {}): TimelineEntry {
  const position = { left: 2, width: 55, clippedStart: false, clippedEnd: false, startLabel: '2026/09/01 09:00', endLabel: '2026/09/18 18:00' };
  return { id: id + ':1', itemId: id, title: id, status: 'OPEN', eventKind: 'ONE_TIME', categoryName: '工作', tagNames: [], shape: 'RANGE', startDate: '2026-09-01', endDate: '2026-09-18', targetDate: null, important: false, occurrenceLabel: null, position, highlight: null, stateSegments: [{ kind: 'ACTIVE', label: '进行中', position }], deadlineMarker: null, completionMarker: null, overdue: false, openEnded: false, historyIncomplete: false, ...overrides };
}
function data(entries: TimelineEntry[] = [], overrides: Partial<TimelineData> = {}): TimelineData {
  return { scale: 'MONTH', rangeStart: '2026-09-01', rangeEnd: '2026-09-30', today: '2026-09-03', ticks: [{ label: '01', position: 0, weekend: false }], todayPosition: 10, totalVisibleCount: entries.length, totalUnscheduledCount: 0, truncated: false, skippedInvalidCount: 0, unscheduled: [], entries, ...overrides };
}
beforeEach(() => {
  vi.spyOn(api, 'saveTimelineView').mockResolvedValue(undefined);
  vi.stubGlobal('ResizeObserver', class { observe() {} unobserve() {} disconnect() {} });
});
afterEach(() => { vi.restoreAllMocks(); vi.useRealTimers(); vi.unstubAllGlobals(); });

describe('TimelineView', () => {
  it('普通事项是创建至截止的横条，活动和预警在区间内高亮', async () => {
    const highlight = { left: 25, width: 20, clippedStart: false, clippedEnd: false, startLabel: '2026/09/08 09:00', endLabel: '2026/09/18 18:00' };
    vi.spyOn(api, 'getTimeline').mockResolvedValue(data([entry('提交审批'), entry('展会筹备', { eventKind: 'CONTINUOUS', highlight }), entry('证书预警', { eventKind: 'WARNING', shape: 'WARNING', highlight })]));
    const open = vi.fn().mockResolvedValue(undefined);
    const { container } = render(TimelineView, { onOpenItem: open });
    await screen.findByText('提交审批');
    expect(container.querySelectorAll('.gantt-bar')).toHaveLength(3);
    expect(container.querySelector('.gantt-bar')).toHaveStyle({ left: '2%', width: '55%' });
    expect(container.querySelectorAll('.gantt-highlight')).toHaveLength(2);
    expect(container.querySelector('.gantt-highlight.warning')).toBeInTheDocument();
    await fireEvent.click(screen.getByRole('button', { name: '打开事项：提交审批' }));
    expect(open).toHaveBeenCalledWith('提交审批');
  });

  it('在同一横条内显示阶段分色、原截止和完成刻点', async () => {
    const whole = { left: 2, width: 70, clippedStart: false, clippedEnd: false, startLabel: '2026/09/01 09:00', endLabel: '2026/09/22 09:00' };
    const active = { ...whole, width: 40, endLabel: '2026/09/13 09:00' };
    const overdue = { ...whole, left: 42, width: 30, startLabel: '2026/09/13 09:00' };
    vi.spyOn(api, 'getTimeline').mockResolvedValue(data([entry('逾期审批', {
      position: whole,
      overdue: true,
      stateSegments: [{ kind: 'ACTIVE', label: '进行中', position: active }, { kind: 'OVERDUE', label: '已逾期', position: overdue }],
      deadlineMarker: { position: 42, label: '截止：2026/09/13 09:00' },
      completionMarker: { position: 65, label: '完成：2026/09/20 09:00' },
    })]));
    const { container } = render(TimelineView);
    await screen.findByText('逾期审批');

    expect(container.querySelector('.gantt-state.active')).toBeInTheDocument();
    expect(container.querySelector('.gantt-state.overdue')).toBeInTheDocument();
    expect(container.querySelector('.gantt-marker.deadline')).toBeInTheDocument();
    expect(container.querySelector('.gantt-marker.completion')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: /阶段：进行中；已逾期/ })).toBeInTheDocument();
  });

  it('同一周期事项合并一行，但保留相邻区间与历史标记', async () => {
    const first = entry('月度对账', { shape: 'OCCURRENCE', historyIncomplete: true });
    vi.spyOn(api, 'getTimeline').mockResolvedValue(data([first, { ...first, id: 'next', position: { ...first.position, left: 57, width: 43 } }]));
    const { container } = render(TimelineView);
    await screen.findByText('月度对账');
    expect(container.querySelectorAll('.gantt-row')).toHaveLength(1);
    expect(container.querySelectorAll('.gantt-bar')).toHaveLength(2);
    expect(screen.getByText(/历史未记录/)).toBeInTheDocument();
  });

  it('空状态仍保留时间待核对事项和返回列表入口', async () => {
    vi.spyOn(api, 'getTimeline').mockResolvedValue(data([], { unscheduled: [{ itemId: 'x', title: '补充活动时间', status: 'OPEN', eventKind: 'CONTINUOUS', reason: '时间信息无法识别' }], totalUnscheduledCount: 1, skippedInvalidCount: 1 }));
    const back = vi.fn();
    render(TimelineView, { onBackToItems: back });
    expect(await screen.findByText('这段时间没有事项')).toBeInTheDocument();
    expect(screen.getByText('补充活动时间')).toBeInTheDocument();
    await fireEvent.click(screen.getByRole('button', { name: '返回列表' }));
    expect(back).toHaveBeenCalled();
  });

  it('首次读取失败说明数据未改变，不暴露技术详情，重试可恢复', async () => {
    vi.spyOn(api, 'getTimeline').mockRejectedValueOnce(new Error('invalid column event_kind at SQL line 4')).mockResolvedValue(data([entry('恢复记录')]));
    render(TimelineView);
    expect(await screen.findByText('甘特图暂时无法读取。事项未被修改，可以重试或返回列表。')).toBeInTheDocument();
    expect(screen.queryByText(/invalid column/)).not.toBeInTheDocument();
    await fireEvent.click(screen.getByRole('button', { name: '重试' }));
    expect(await screen.findByText('恢复记录')).toBeInTheDocument();
    expect(screen.queryByRole('alert')).not.toBeInTheDocument();
  });

  it('刷新失败保留旧区间及真实月份，不能把旧结果标成新月份', async () => {
    vi.spyOn(api, 'getTimeline').mockResolvedValueOnce(data([entry('已有记录')])).mockRejectedValue(new Error('IPC'));
    render(TimelineView);
    await screen.findByText('已有记录');
    await fireEvent.click(screen.getByRole('button', { name: '下一段时间' }));
    expect(await screen.findByText(/仍显示上次读取的区间/)).toBeInTheDocument();
    expect(screen.getByRole('heading', { name: '2026 年 9 月' })).toBeInTheDocument();
    expect(screen.getByText('已有记录')).toBeInTheDocument();
  });

  it('过期响应不覆盖后来选择的视图，偏好保存失败不丢失数据', async () => {
    let resolveFirst!: (value: TimelineData) => void;
    vi.spyOn(api, 'getTimeline').mockImplementationOnce(() => new Promise((resolve) => { resolveFirst = resolve; })).mockResolvedValue(data([entry('新视图')], { scale: 'WEEK' }));
    vi.mocked(api.saveTimelineView).mockRejectedValue(new Error('read-only'));
    render(TimelineView);
    await fireEvent.click(screen.getByRole('button', { name: '周' }));
    await screen.findByText('新视图');
    resolveFirst(data([entry('过期视图')]));
    await waitFor(() => expect(screen.queryByText('过期视图')).not.toBeInTheDocument());
    expect(screen.queryByRole('alert')).not.toBeInTheDocument();
  });

  it('月末导航不跳过二月，筛选按用户选择提交', async () => {
    vi.useFakeTimers({ toFake: ['Date'] });
    vi.setSystemTime(new Date('2026-01-31T12:00:00+08:00'));
    const read = vi.spyOn(api, 'getTimeline').mockResolvedValue(data());
    render(TimelineView, { categories: [{ id: 'work', name: '工作', color: '#315e91' }], tags: [] });
    await waitFor(() => expect(read).toHaveBeenCalledTimes(1));
    await fireEvent.click(screen.getByRole('button', { name: '下一段时间' }));
    expect(read).toHaveBeenLastCalledWith(expect.objectContaining({ anchorDate: '2026-02-01' }));
    await fireEvent.click(screen.getByRole('button', { name: '筛选' }));
    await fireEvent.change(screen.getByLabelText('分类'), { target: { value: 'work' } });
    await fireEvent.click(screen.getByLabelText('显示已完成'));
    await fireEvent.click(screen.getByRole('button', { name: '查看结果' }));
    expect(read).toHaveBeenLastCalledWith(expect.objectContaining({ categoryId: 'work', includeDone: true }));
  });

  it('大量事项只渲染可见行，方向键可移动焦点，组合态不移动', async () => {
    vi.spyOn(api, 'getTimeline').mockResolvedValue(data(Array.from({ length: 2000 }, (_, i) => entry('事项' + i))));
    const { container } = render(TimelineView);
    const first = await screen.findByRole('button', { name: '打开事项：事项0' });
    expect(container.querySelectorAll('.gantt-row').length).toBeLessThan(35);
    first.focus();
    await fireEvent.keyDown(first, { key: 'ArrowDown', isComposing: true });
    expect(first).toHaveFocus();
    await fireEvent.keyDown(first, { key: 'ArrowDown' });
    expect(screen.getByRole('button', { name: '打开事项：事项1' })).toHaveFocus();
    await fireEvent.keyDown(document.activeElement!, { key: 'End' });
    expect(await screen.findByRole('button', { name: '打开事项：事项1999' })).toHaveFocus();
  });

  it('详情和快速记录打开失败均提供恢复入口，不展示底层错误', async () => {
    vi.spyOn(api, 'getTimeline').mockResolvedValue(data([entry('待打开')]));
    vi.spyOn(api, 'showCapture').mockRejectedValue(new Error('state not managed'));
    render(TimelineView, { onOpenItem: vi.fn().mockRejectedValue(new Error('SQL')) });
    await fireEvent.click(await screen.findByRole('button', { name: '打开事项：待打开' }));
    expect(await screen.findByText(/事项暂时无法打开/)).toBeInTheDocument();
    await fireEvent.click(screen.getByRole('button', { name: /快速记录/ }));
    expect(await screen.findByText(/快速记录窗口未能打开/)).toBeInTheDocument();
    expect(screen.queryByText(/state not managed|SQL/)).not.toBeInTheDocument();
  });
});
