import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@testing-library/svelte';
import { ApiError } from '$lib/api/_base.svelte';

const createTemplate = vi.fn();
const newTemplateVersion = vi.fn();

vi.mock('$lib/api/collabOps', async (orig) => ({
  ...(await orig<typeof import('$lib/api/collabOps')>()),
  collabOps: {
    createTemplate: (...a: unknown[]) => createTemplate(...a),
    newTemplateVersion: (...a: unknown[]) => newTemplateVersion(...a),
  },
}));
vi.mock('$lib/components/toast.svelte', () => ({ toast: { success: vi.fn(), error: vi.fn() } }));
vi.mock('$lib/i18n', () => {
  const t = (key: string, params?: Record<string, unknown>) =>
    params && typeof params === 'object' ? `${key} ${JSON.stringify(params)}` : key;
  return { createT: () => t, t };
});

import TemplateForm from './TemplateForm.svelte';
import type { TemplateView } from '$lib/api/collabOps';

function tpl(over: Partial<TemplateView> = {}): TemplateView {
  return {
    id: 'tpl-1',
    name: 'Bugfix',
    latest_version: 2,
    created_by_user_id: 1,
    created_at: '2026-11-05T00:00:00Z',
    updated_at: '2026-11-05T00:00:00Z',
    origin: 'user_content',
    version: {
      template_id: 'tpl-1',
      version: 2,
      title_pattern: 'Fix {bug}',
      goal: 'Find and fix it',
      deliverables: ['patch'],
      checklist: [{ text: 'tests pass', required: true }],
      created_by_user_id: 1,
      created_at: '2026-11-05T00:00:00Z',
      origin: 'user_content',
    },
    ...over,
  };
}

const byLabel = (key: string) => screen.getByLabelText(new RegExp(`^${key}`)) as HTMLInputElement;

function mount(template: TemplateView | null, extra: Record<string, unknown> = {}) {
  const onSaved = vi.fn();
  const onCancel = vi.fn();
  render(TemplateForm, { props: { owner: 'alice', repo: 'demo', template, onSaved, onCancel, ...extra } });
  return { onSaved, onCancel };
}

describe('TemplateForm.svelte', () => {
  beforeEach(() => {
    createTemplate.mockReset();
    newTemplateVersion.mockReset();
  });

  it('creates a template with a client id and the parsed lists', async () => {
    createTemplate.mockResolvedValue(tpl());
    const { onSaved } = mount(null);
    await fireEvent.input(byLabel('collab_ops.templates.name'), { target: { value: '  Bugfix ' } });
    await fireEvent.input(byLabel('collab_ops.templates.title_pattern'), { target: { value: 'Fix {bug}' } });
    await fireEvent.input(byLabel('collab_ops.templates.goal'), { target: { value: 'Find and fix it' } });
    await fireEvent.input(byLabel('collab_ops.templates.deliverables'), { target: { value: 'patch\n\n notes ' } });
    await fireEvent.input(byLabel('collab_ops.templates.checklist'), { target: { value: 'tests pass\n[optional] docs' } });
    await fireEvent.click(screen.getByRole('button', { name: 'collab_ops.templates.create' }));
    await waitFor(() => expect(createTemplate).toHaveBeenCalledTimes(1));
    const [owner, repo, input] = createTemplate.mock.calls[0];
    expect([owner, repo]).toEqual(['alice', 'demo']);
    expect(input).toMatchObject({
      name: 'Bugfix',
      title_pattern: 'Fix {bug}',
      goal: 'Find and fix it',
      deliverables: ['patch', 'notes'],
      checklist: [
        { text: 'tests pass', required: true },
        { text: 'docs', required: false },
      ],
    });
    expect(input.client_template_id).toBeTruthy();
    await waitFor(() => expect(onSaved).toHaveBeenCalled());
  });

  it('blocks submit and explains the problem for a bad title pattern', async () => {
    mount(null);
    await fireEvent.input(byLabel('collab_ops.templates.name'), { target: { value: 'X' } });
    await fireEvent.input(byLabel('collab_ops.templates.title_pattern'), { target: { value: 'Fix {' } });
    await fireEvent.input(byLabel('collab_ops.templates.goal'), { target: { value: 'g' } });
    expect(screen.getByTestId('template-problem')).toHaveTextContent('collab_ops.templates.problem.title_braces');
    expect(screen.getByRole('button', { name: 'collab_ops.templates.create' })).toBeDisabled();
  });

  it('saves an edit as a new version carrying the version the editor saw', async () => {
    newTemplateVersion.mockResolvedValue(tpl({ latest_version: 3 }));
    mount(tpl());
    // nothing changed yet: no empty "new version"
    expect(screen.getByRole('button', { name: 'collab_ops.templates.save_version' })).toBeDisabled();
    expect(screen.getByTestId('template-unchanged')).toBeInTheDocument();
    await fireEvent.input(byLabel('collab_ops.templates.goal'), { target: { value: 'Changed goal' } });
    await fireEvent.click(screen.getByRole('button', { name: 'collab_ops.templates.save_version' }));
    await waitFor(() => expect(newTemplateVersion).toHaveBeenCalledTimes(1));
    const [, , id, input] = newTemplateVersion.mock.calls[0];
    expect(id).toBe('tpl-1');
    expect(input).toMatchObject({ expected_version: 2, goal: 'Changed goal', title_pattern: 'Fix {bug}' });
    expect(input.client_version_id).toBeTruthy();
  });

  it('uses a NEW idempotency key after a failed attempt', async () => {
    createTemplate.mockRejectedValueOnce(new ApiError('name taken', 409, 'TEMPLATE_NAME_TAKEN')).mockResolvedValueOnce(tpl());
    mount(null);
    await fireEvent.input(byLabel('collab_ops.templates.name'), { target: { value: 'Bugfix' } });
    await fireEvent.input(byLabel('collab_ops.templates.title_pattern'), { target: { value: 'Fix' } });
    await fireEvent.input(byLabel('collab_ops.templates.goal'), { target: { value: 'g' } });
    await fireEvent.click(screen.getByRole('button', { name: 'collab_ops.templates.create' }));
    expect(await screen.findByText(/name taken/)).toBeInTheDocument();
    await fireEvent.click(screen.getByRole('button', { name: 'collab_ops.templates.create' }));
    await waitFor(() => expect(createTemplate).toHaveBeenCalledTimes(2));
    expect(createTemplate.mock.calls[1][2].client_template_id).not.toBe(createTemplate.mock.calls[0][2].client_template_id);
  });

  it('shows the server message and a reload action on VERSION_CONFLICT', async () => {
    newTemplateVersion.mockRejectedValue(new ApiError('template changed', 409, 'VERSION_CONFLICT'));
    const onConflict = vi.fn();
    mount(tpl(), { onConflict });
    await fireEvent.input(byLabel('collab_ops.templates.goal'), { target: { value: 'Changed goal' } });
    await fireEvent.click(screen.getByRole('button', { name: 'collab_ops.templates.save_version' }));
    expect(await screen.findByText(/template changed/)).toBeInTheDocument();
    expect(screen.getByTestId('template-conflict')).toBeInTheDocument();
    await fireEvent.click(screen.getByRole('button', { name: 'collab_ops.templates.reload' }));
    expect(onConflict).toHaveBeenCalled();
  });

  it('keeps hostile text in the fields as text and sends it unchanged', async () => {
    const hostile = '<img src=x onerror="window.__pwned=1"><script>window.__pwned=1</script>';
    mount(tpl({ version: { ...tpl().version, goal: hostile } }));
    expect(byLabel('collab_ops.templates.goal').value).toBe(hostile);
    expect(document.querySelector('[data-testid="template-form"] img')).toBeNull();
    expect(document.querySelector('[data-testid="template-form"] script')).toBeNull();
    expect((window as unknown as { __pwned?: number }).__pwned).toBeUndefined();
  });
});
