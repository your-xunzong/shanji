import { fireEvent, render, screen } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';
import ExportPanel from './ExportPanel.svelte';
import { api } from '../lib/api';

describe('ExportPanel', () => {
  it('submits status, type and tag filters before choosing a file', async () => {
    const onExport = vi.fn(async () => {});
    render(ExportPanel, { props: {
      categories: [{ id: 'work', name: '工作', color: '#627D98' }],
      tags: [{ id: 'customer', name: '客户', color: '#B06C49' }],
      onClose: vi.fn(),
      onExport,
    } });

    await fireEvent.change(screen.getByLabelText('事项状态'), { target: { value: 'open' } });
    await fireEvent.change(screen.getByLabelText('事件类型'), { target: { value: 'ORDINARY' } });
    await fireEvent.change(screen.getByLabelText('类型'), { target: { value: 'work' } });
    await fireEvent.change(screen.getByLabelText('标签'), { target: { value: 'customer' } });
    await fireEvent.click(screen.getByRole('button', { name: '选择位置并导出' }));

    expect(onExport).toHaveBeenCalledWith({
      status: 'open',
      eventKind: 'ORDINARY',
      categoryId: 'work',
      tagId: 'customer',
      createdFrom: null,
      createdTo: null,
    });
  });

  it('validates a Word template before generating a monthly summary', async () => {
    vi.spyOn(api, 'validateSummaryTemplate').mockResolvedValue({
      templatePath: 'D:\\模板\\月报.docx',
      placeholders: ['report_title', 'open_item.title'],
      unsupportedPlaceholders: [],
      valid: true,
      message: '已识别 2 个占位符，可以生成小结。',
    });
    const onGenerateSummary = vi.fn(async () => {});
    render(ExportPanel, { props: {
      categories: [], tags: [],
      onClose: vi.fn(), onExport: vi.fn(),
      onChooseSummaryTemplate: vi.fn(async () => 'D:\\模板\\月报.docx'),
      onGenerateSummary,
    } });
    await fireEvent.click(screen.getByRole('tab', { name: 'Word 小结' }));
    await fireEvent.click(screen.getByRole('button', { name: '选择模板' }));
    expect(await screen.findByText(/已识别 2 个占位符/)).toBeInTheDocument();
    await fireEvent.click(screen.getByRole('button', { name: '选择位置并生成' }));
    expect(onGenerateSummary).toHaveBeenCalledWith(expect.objectContaining({
      period: 'MONTH', templatePath: 'D:\\模板\\月报.docx', outputPath: '',
    }));
  });
});
