import { fireEvent, render, screen } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';
import OrganizerPanel from './OrganizerPanel.svelte';

function props() {
  return {
    categories: [{ id: 'work', name: '工作', color: '#627D98' }],
    tags: [],
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

  it('asks where used items should move before deleting a type', async () => {
    render(OrganizerPanel, { props: props() });
    await fireEvent.click(screen.getByRole('button', { name: '删除' }));
    expect(screen.getByRole('combobox', { name: '删除类型后的迁移目标' })).toBeInTheDocument();
    expect(screen.getByText('事项不会被删除。请选择这些事项之后使用的类型。')).toBeInTheDocument();
  });

  it('lets the user reorder types with named controls', async () => {
    const values = props();
    values.categories.push({ id: 'personal', name: '个人', color: '#6E8B74' });
    render(OrganizerPanel, { props: values });
    await fireEvent.click(screen.getByRole('button', { name: '上移类型：个人' }));
    expect(values.onMoveCategory).toHaveBeenCalledWith('personal', 'up');
  });
});
