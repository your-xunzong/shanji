import { fireEvent, render, screen } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';
import type { Category, Tag } from '../lib/types';
import OrganizerPanel from './OrganizerPanel.svelte';

function props() {
  return {
    categories: [] as Category[],
    tags: [] as Tag[],
    onClose: vi.fn(),
    onCreateCategory: vi.fn(async () => {}),
    onUpdateCategory: vi.fn(async () => {}),
    onDeleteCategory: vi.fn(async () => {}),
    onMoveCategory: vi.fn(async () => {}),
    onCreateTag: vi.fn(async () => {}),
    onUpdateTag: vi.fn(async () => {}),
    onDeleteTag: vi.fn(async () => {}),
    onMoveTag: vi.fn(async () => {}),
  };
}

describe('OrganizerPanel', () => {
  it('adds a colored tag with user-facing controls', async () => {
    const values = props();
    render(OrganizerPanel, { props: values });
    await fireEvent.input(screen.getByLabelText('新标签名称'), { target: { value: '客户' } });
    await fireEvent.click(screen.getByRole('button', { name: '添加标签' }));
    expect(values.onCreateTag).toHaveBeenCalledWith({ name: '客户', color: '#B06C49' });
  });

  it('管理单选类型，不与标签混用', async () => {
    const values = props();
    await fireEvent.input(render(OrganizerPanel, { props: values }).getByLabelText('新类型名称'), { target: { value: '学习' } });
    await fireEvent.click(screen.getByRole('button', { name: '添加类型' }));
    expect(values.onCreateCategory).toHaveBeenCalledWith({ name: '学习', color: '#627D98' });
  });

  it('shows seven fixed event kinds without exposing delete controls', () => {
    render(OrganizerPanel, { props: props() });
    expect(screen.getByText('普通')).toBeInTheDocument();
    expect(screen.getByText('一次性')).toBeInTheDocument();
    expect(screen.getByText('年度')).toBeInTheDocument();
  });

  it('lets the user reorder labels with named controls', async () => {
    const values = props();
    values.tags.push(
      { id: 'work', name: '工作', color: '#627D98' },
      { id: 'personal', name: '个人', color: '#6E8B74' },
    );
    render(OrganizerPanel, { props: values });
    await fireEvent.click(screen.getByRole('button', { name: '上移标签：个人' }));
    expect(values.onMoveTag).toHaveBeenCalledWith('personal', 'up');
  });
});
