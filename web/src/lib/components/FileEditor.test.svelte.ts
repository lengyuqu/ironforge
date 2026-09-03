import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@testing-library/svelte';

vi.mock('$lib/i18n', () => ({
  createT: () => (key: string, fallbackOrParams?: string | Record<string, unknown>) =>
    typeof fallbackOrParams === 'string' ? fallbackOrParams : key,
  formatDate: (iso: string) => `fmt(${iso})`,
  formatDateTime: (iso: string) => `fmtt(${iso})`,
}));

// Deterministic markdown + syntax highlighting — both are pure output concerns
// that the editor just pipes into the DOM.
vi.mock('$lib/utils/markdown', () => ({
  renderMarkdown: (content: string) => `<p>md(${content})</p>`,
}));
vi.mock('highlight.js', () => ({
  default: {
    getLanguage: () => null,
    highlightAuto: (content: string) => ({ value: content }),
    highlight: (content: string) => ({ value: content }),
  },
}));

import FileEditor from './FileEditor.svelte';

const onSave = vi.fn();

beforeEach(() => {
  onSave.mockReset();
  onSave.mockResolvedValue(undefined);
});

function renderEditor(overrides: Record<string, unknown> = {}) {
  return render(FileEditor, {
    props: {
      owner: 'alice',
      repo: 'demo',
      mode: 'create',
      initialPath: 'docs/guide.md',
      initialContent: '',
      cancelHref: '/alice/demo',
      onSave,
      ...overrides,
    },
  });
}

const pathInput = () => screen.getByLabelText('repo.editor.path') as HTMLInputElement;
const messageInput = () => screen.getByLabelText('repo.editor.commit_message') as HTMLInputElement;
const branchInput = () => screen.getByLabelText('repo.editor.branch') as HTMLInputElement;
const contentArea = () => screen.getByPlaceholderText('repo.editor.content_placeholder') as HTMLTextAreaElement;
const saveButton = () => screen.getByRole('button', { name: /repo\.editor\.(create_file|save_changes|saving)/ });

async function setContent(value: string) {
  fireEvent.input(contentArea(), { target: { value } });
  await waitFor(() => expect(contentArea().value).toBe(value));
}

describe('FileEditor.svelte', () => {
  it('seeds create mode from the initial props', async () => {
    renderEditor({ mode: 'create', initialPath: 'docs/guide.md', branch: 'develop' });

    expect(screen.getByText('alice/demo')).toBeInTheDocument();
    expect(screen.getByText('repo.new_file')).toBeInTheDocument();
    expect(pathInput().value).toBe('docs/guide.md');
    expect(pathInput().readOnly).toBe(false);
    // Commit message is auto-derived from the path so the user can just save.
    expect(messageInput().value).toBe('Create docs/guide.md');
    expect(branchInput().value).toBe('develop');
  });

  it('locks the path and prefills content in edit mode', async () => {
    renderEditor({ mode: 'edit', initialPath: 'src/main.rs', initialContent: 'fn main() {}' });

    expect(screen.getByText('repo.edit_file')).toBeInTheDocument();
    expect(pathInput().readOnly).toBe(true);
    await waitFor(() => expect(contentArea().value).toBe('fn main() {}'));
    expect(messageInput().value).toBe('Update src/main.rs');
  });

  it('keeps save disabled in edit mode until the content actually changes', async () => {
    renderEditor({ mode: 'edit', initialContent: 'one\ntwo' });

    expect(saveButton()).toBeDisabled();

    await setContent('one\ntwo\nthree');
    expect(saveButton()).toBeEnabled();
  });

  it('submits the trimmed payload, carrying sha only when editing', async () => {
    renderEditor({ mode: 'edit', initialPath: 'a.txt', initialSha: 'abc123', branch: ' main ' });

    await setContent('hello');
    fireEvent.input(messageInput(), { target: { value: '  tweak  ' } });
    fireEvent.click(saveButton());

    await waitFor(() => expect(onSave).toHaveBeenCalledTimes(1));
    expect(onSave).toHaveBeenCalledWith({
      path: 'a.txt',
      content: 'hello',
      message: 'tweak',
      branch: 'main',
      sha: 'abc123',
    });
  });

  it('omits sha and blocks submission when a required field is empty', async () => {
    renderEditor({ mode: 'create', initialPath: 'new.txt' });
    await setContent('body');

    fireEvent.input(messageInput(), { target: { value: '   ' } });
    fireEvent.click(saveButton());
    expect(await screen.findByText('repo.editor.message_required')).toBeInTheDocument();
    expect(onSave).not.toHaveBeenCalled();

    fireEvent.input(messageInput(), { target: { value: 'add file' } });
    fireEvent.input(branchInput(), { target: { value: '  ' } });
    fireEvent.click(saveButton());
    expect(await screen.findByText('repo.editor.branch_required')).toBeInTheDocument();
    expect(onSave).not.toHaveBeenCalled();
  });

  it('rejects malformed paths before hitting the API', async () => {
    renderEditor({ mode: 'create', initialPath: '' });
    await setContent('body');

    for (const bad of ['/abs/path', 'a\\\\b', 'a//b', 'a/../b']) {
      fireEvent.input(pathInput(), { target: { value: bad } });
      fireEvent.click(saveButton());
      expect(await screen.findByText(/repo\.editor\.path_invalid/)).toBeInTheDocument();
    }
    expect(onSave).not.toHaveBeenCalled();
  });

  it('switches between preview and diff tabs', async () => {
    renderEditor({ mode: 'edit', initialPath: 'notes.md', initialContent: 'line one' });
    await waitFor(() => expect(contentArea().value).toBe('line one'));

    fireEvent.click(screen.getByRole('button', { name: 'repo.editor.preview_tab' }));
    expect(await screen.findByText('md(line one)')).toBeInTheDocument();

    fireEvent.click(screen.getByRole('button', { name: 'repo.editor.diff_tab' }));
    // Nothing changed yet — the diff pane collapses to a placeholder.
    expect(await screen.findByText('repo.editor.no_changes')).toBeInTheDocument();

    // The textarea only exists on the edit tab, so go back before typing.
    fireEvent.click(screen.getByRole('button', { name: 'repo.editor.edit_tab' }));
    await setContent('line one\nline two');

    fireEvent.click(screen.getByRole('button', { name: 'repo.editor.diff_tab' }));
    await waitFor(() => expect(screen.getByText('+')).toBeInTheDocument());
    expect(screen.getByText('line two')).toBeInTheDocument();
  });

  it('renders non-markdown previews as plain code', async () => {
    renderEditor({ mode: 'edit', initialPath: 'main.rs', initialContent: 'fn main() {}' });
    await waitFor(() => expect(contentArea().value).toBe('fn main() {}'));

    fireEvent.click(screen.getByRole('button', { name: 'repo.editor.preview_tab' }));
    const pre = await screen.findByText('fn main() {}');
    expect(pre.tagName).toBe('CODE');
  });

  it('surfaces save failures, offering a reload only on conflicts', async () => {
    renderEditor();
    await setContent('body');

    onSave.mockRejectedValueOnce(new Error('sha mismatch'));
    fireEvent.click(saveButton());

    await waitFor(() => expect(screen.getByText('repo.editor.conflict')).toBeInTheDocument());
    expect(screen.getByText('repo.editor.reload_latest')).toBeInTheDocument();

    onSave.mockRejectedValueOnce(new Error('boom'));
    fireEvent.click(saveButton());

    await waitFor(() => expect(screen.getByText('repo.editor.save_failed')).toBeInTheDocument());
    expect(screen.queryByText('repo.editor.reload_latest')).not.toBeInTheDocument();
  });

  it('shows the disabled reason and blocks interaction', async () => {
    renderEditor({ mode: 'edit', initialContent: 'x', disabledReason: 'read-only mirror' });

    expect(screen.getByText('read-only mirror')).toBeInTheDocument();
    expect(saveButton()).toBeDisabled();
    expect(contentArea()).toBeDisabled();
  });
});
