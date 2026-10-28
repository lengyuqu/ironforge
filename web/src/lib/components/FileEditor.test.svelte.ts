import { describe, it, expect, vi } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@testing-library/svelte';

import FileEditor from './FileEditor.svelte';

type Props = Partial<{
  mode: 'create' | 'edit';
  initialPath: string;
  initialContent: string;
  initialSha: string;
  branch: string;
  disabledReason: string;
  onSave: (p: unknown) => Promise<void>;
}>;

function renderEditor(props: Props = {}) {
  const onSave = props.onSave ?? vi.fn().mockResolvedValue(undefined);
  render(FileEditor, {
    owner: 'alice',
    repo: 'demo',
    mode: 'create',
    cancelHref: '/alice/demo',
    ...props,
    onSave,
  });
  return { onSave: onSave as ReturnType<typeof vi.fn> };
}

const pathInput = () => screen.getByLabelText('File path') as HTMLInputElement;
const content = () => document.getElementById('file-content') as HTMLTextAreaElement;
const message = () => screen.getByLabelText('Commit message') as HTMLInputElement;
const branchInput = () => screen.getByLabelText('Branch') as HTMLInputElement;
const saveButton = (name: string) => screen.getByRole('button', { name });

describe('FileEditor.svelte', () => {
  describe('initial state', () => {
    it('create mode starts empty with a generic commit message and the default branch', () => {
      renderEditor();

      expect(screen.getByText('alice/demo')).toBeInTheDocument();
      expect(screen.getByRole('heading', { name: 'New File' })).toBeInTheDocument();
      expect(pathInput().value).toBe('');
      expect(pathInput()).not.toHaveAttribute('readonly');
      expect(message().value).toBe('Create new file');
      expect(branchInput().value).toBe('main');
    });

    it('edit mode locks the path and derives the commit message from it', () => {
      renderEditor({ mode: 'edit', initialPath: 'docs/a.md', initialContent: 'hi', branch: 'dev' });

      expect(screen.getByRole('heading', { name: 'Edit File' })).toBeInTheDocument();
      expect(pathInput().value).toBe('docs/a.md');
      expect(pathInput()).toHaveAttribute('readonly');
      expect(content().value).toBe('hi');
      expect(message().value).toBe('Update docs/a.md');
      expect(branchInput().value).toBe('dev');
    });

    it('links both cancel buttons to cancelHref', () => {
      renderEditor();
      const links = screen.getAllByRole('link', { name: 'Cancel' });
      expect(links).toHaveLength(2);
      for (const l of links) expect(l).toHaveAttribute('href', '/alice/demo');
    });
  });

  describe('save button state', () => {
    it('edit mode only enables saving once the content changed', async () => {
      renderEditor({ mode: 'edit', initialPath: 'a.txt', initialContent: 'one' });
      expect(saveButton('Save changes')).toBeDisabled();

      await fireEvent.input(content(), { target: { value: 'two' } });
      expect(saveButton('Save changes')).toBeEnabled();

      await fireEvent.input(content(), { target: { value: 'one' } });
      expect(saveButton('Save changes')).toBeDisabled();
    });

    it('create mode is always enabled', () => {
      renderEditor();
      expect(saveButton('Create file')).toBeEnabled();
    });

    it('a disabledReason shows a warning, locks the textarea and the save button', async () => {
      const { onSave } = renderEditor({ disabledReason: 'Branch is protected' });

      expect(screen.getByText('Branch is protected')).toBeInTheDocument();
      expect(content()).toBeDisabled();
      expect(saveButton('Create file')).toBeDisabled();
      expect(onSave).not.toHaveBeenCalled();
    });
  });

  describe('validation', () => {
    const cases: Array<[string, string]> = [
      ['', 'File path is required'],
      ['   ', 'File path is required'],
      ['/abs.txt', 'File path cannot start with a slash.'],
      ['\\abs.txt', 'File path cannot start with a slash.'],
      ['a\\b.txt', 'Use forward slashes in file paths.'],
      ['a//b.txt', 'File path cannot contain empty folders.'],
      ['dir/', 'File path cannot contain empty folders.'],
      ['a/./b', 'File path cannot contain . or .. segments.'],
      ['../b', 'File path cannot contain . or .. segments.'],
      ['a\u0001b', 'File path contains invalid characters.'],
    ];

    it.each(cases)('rejects path %j', async (value, expected) => {
      const { onSave } = renderEditor();
      await fireEvent.input(pathInput(), { target: { value } });
      await fireEvent.click(saveButton('Create file'));

      expect(screen.getByText(expected)).toBeInTheDocument();
      expect(onSave).not.toHaveBeenCalled();
    });

    it('requires a commit message and a branch', async () => {
      const { onSave } = renderEditor();
      await fireEvent.input(pathInput(), { target: { value: 'a.txt' } });

      await fireEvent.input(message(), { target: { value: '  ' } });
      await fireEvent.click(saveButton('Create file'));
      expect(screen.getByText('Commit message is required')).toBeInTheDocument();

      await fireEvent.input(message(), { target: { value: 'msg' } });
      await fireEvent.input(branchInput(), { target: { value: ' ' } });
      await fireEvent.click(saveButton('Create file'));
      expect(screen.getByText('Branch is required')).toBeInTheDocument();
      expect(onSave).not.toHaveBeenCalled();
    });
  });

  describe('saving', () => {
    it('create mode sends trimmed fields and no sha', async () => {
      const { onSave } = renderEditor({ initialSha: 'ignored' });
      await fireEvent.input(pathInput(), { target: { value: ' docs/new.md ' } });
      await fireEvent.input(content(), { target: { value: 'body' } });
      await fireEvent.input(message(), { target: { value: ' add doc ' } });
      await fireEvent.input(branchInput(), { target: { value: ' feature ' } });
      await fireEvent.click(saveButton('Create file'));

      await waitFor(() => expect(onSave).toHaveBeenCalledTimes(1));
      expect(onSave).toHaveBeenCalledWith({
        path: 'docs/new.md',
        content: 'body',
        message: 'add doc',
        branch: 'feature',
      });
    });

    it('edit mode includes the original sha', async () => {
      const { onSave } = renderEditor({
        mode: 'edit',
        initialPath: 'a.txt',
        initialContent: 'one',
        initialSha: 'abc123',
      });
      await fireEvent.input(content(), { target: { value: 'two' } });
      await fireEvent.click(saveButton('Save changes'));

      await waitFor(() => expect(onSave).toHaveBeenCalledTimes(1));
      expect(onSave).toHaveBeenCalledWith({
        path: 'a.txt',
        content: 'two',
        message: 'Update a.txt',
        branch: 'main',
        sha: 'abc123',
      });
    });

    it('shows the saving label while the request is pending', async () => {
      let resolve!: () => void;
      const onSave = vi.fn(() => new Promise<void>((r) => (resolve = r)));
      renderEditor({ onSave });
      await fireEvent.input(pathInput(), { target: { value: 'a.txt' } });
      await fireEvent.click(saveButton('Create file'));

      expect(await screen.findByRole('button', { name: 'Saving...' })).toBeDisabled();
      resolve();
      await waitFor(() => expect(saveButton('Create file')).toBeEnabled());
    });

    it('reports a generic failure with the error message', async () => {
      renderEditor({ onSave: vi.fn().mockRejectedValue(new Error('disk full')) });
      await fireEvent.input(pathInput(), { target: { value: 'a.txt' } });
      await fireEvent.click(saveButton('Create file'));

      expect(await screen.findByText('Save failed: disk full')).toBeInTheDocument();
      expect(screen.queryByRole('button', { name: 'Reload latest' })).toBeNull();
    });

    it('treats non-Error rejections as text', async () => {
      renderEditor({ onSave: vi.fn().mockRejectedValue('nope') });
      await fireEvent.input(pathInput(), { target: { value: 'a.txt' } });
      await fireEvent.click(saveButton('Create file'));

      expect(await screen.findByText('Save failed: nope')).toBeInTheDocument();
    });

    it.each(['sha mismatch on write', 'Merge CONFLICT detected'])(
      'recognises %j as an edit conflict and offers a reload',
      async (msg) => {
        renderEditor({ onSave: vi.fn().mockRejectedValue(new Error(msg)) });
        await fireEvent.input(pathInput(), { target: { value: 'a.txt' } });
        await fireEvent.click(saveButton('Create file'));

        expect(
          await screen.findByText(
            'This file changed since you opened it. Reload the latest version before saving.'
          )
        ).toBeInTheDocument();
        expect(screen.getByRole('button', { name: 'Reload latest' })).toBeInTheDocument();
      }
    );

    it('clears a previous error on the next attempt', async () => {
      const onSave = vi.fn().mockRejectedValueOnce(new Error('boom')).mockResolvedValue(undefined);
      renderEditor({ onSave });
      await fireEvent.input(pathInput(), { target: { value: 'a.txt' } });
      await fireEvent.click(saveButton('Create file'));
      await screen.findByText('Save failed: boom');

      await fireEvent.click(saveButton('Create file'));
      await waitFor(() => expect(screen.queryByText('Save failed: boom')).toBeNull());
    });
  });

  describe('tabs', () => {
    it('shows the editor by default', () => {
      renderEditor();
      expect(content()).toBeInTheDocument();
      expect(screen.getByRole('tablist', { name: 'File editor views' })).toBeInTheDocument();
    });

    it('previews markdown files as rendered HTML', async () => {
      renderEditor({ initialPath: 'README.md' });
      await fireEvent.input(content(), { target: { value: '# Title\n\nhello' } });
      await fireEvent.click(screen.getByRole('button', { name: 'Preview' }));

      expect(document.querySelector('.markdown-body h1')?.textContent).toContain('Title');
      expect(document.getElementById('file-content')).toBeNull();
    });

    it('previews non-markdown files verbatim', async () => {
      renderEditor({ initialPath: 'a.txt' });
      await fireEvent.input(content(), { target: { value: '# not markdown' } });
      await fireEvent.click(screen.getByRole('button', { name: 'Preview' }));

      expect(document.querySelector('.markdown-body')).toBeNull();
      expect(document.querySelector('.preview-pane pre code')?.textContent).toBe('# not markdown');
    });

    it('shows an empty-file hint in the preview when there is no content', async () => {
      renderEditor({ initialPath: 'a.txt' });
      await fireEvent.click(screen.getByRole('button', { name: 'Preview' }));
      expect(screen.getByText('This file is empty.')).toBeInTheDocument();
    });

    it('diff tab lists added and removed lines with line numbers', async () => {
      renderEditor({ mode: 'edit', initialPath: 'a.txt', initialContent: 'one\ntwo' });
      await fireEvent.input(content(), { target: { value: 'one\nthree' } });
      await fireEvent.click(screen.getByRole('button', { name: 'Diff' }));

      const rows = Array.from(document.querySelectorAll('.diff-pane tr')).map((tr) => ({
        cls: tr.classList[0],
        cells: Array.from(tr.querySelectorAll('td')).map((td) => td.textContent),
      }));
      expect(rows).toEqual([
        { cls: 'same', cells: ['1', '1', ' ', 'one'] },
        { cls: 'del', cells: ['2', '', '-', 'two'] },
        { cls: 'add', cells: ['', '2', '+', 'three'] },
      ]);
    });

    it('diff tab says so when nothing changed in edit mode', async () => {
      renderEditor({ mode: 'edit', initialPath: 'a.txt', initialContent: 'one' });
      await fireEvent.click(screen.getByRole('button', { name: 'Diff' }));
      expect(screen.getByText('No changes.')).toBeInTheDocument();
    });

    it('switching back to Edit keeps the typed content', async () => {
      renderEditor();
      await fireEvent.input(content(), { target: { value: 'kept' } });
      await fireEvent.click(screen.getByRole('button', { name: 'Preview' }));
      await fireEvent.click(screen.getByRole('button', { name: 'Edit' }));
      expect(content().value).toBe('kept');
    });
  });

  describe('syntax highlighting backdrop', () => {
    it('highlights using the language implied by the file extension', async () => {
      renderEditor({ initialPath: 'main.rs' });
      await fireEvent.input(content(), { target: { value: 'fn main() {}' } });

      const code = document.querySelector('.highlight-backdrop code') as HTMLElement;
      expect(code.className).toContain('language-rust');
      await waitFor(() => expect(code.innerHTML).toContain('hljs-keyword'));
    });

    it('falls back to no language class for unknown extensions', () => {
      renderEditor({ initialPath: 'notes.unknownext' });
      const code = document.querySelector('.highlight-backdrop code') as HTMLElement;
      expect(Array.from(code.classList).slice(0, 2)).toEqual(['hljs', 'language-']);
    });

    it('mirrors the textarea scroll position onto the backdrop', async () => {
      renderEditor();
      const area = content();
      const backdrop = document.querySelector('.highlight-backdrop') as HTMLElement;
      area.scrollTop = 40;
      area.scrollLeft = 7;
      await fireEvent.scroll(area);
      expect(backdrop.scrollTop).toBe(40);
      expect(backdrop.scrollLeft).toBe(7);
    });
  });
});
