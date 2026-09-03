import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@testing-library/svelte';

const attachmentsList = vi.fn();
const attachmentsUpload = vi.fn();
const attachmentsRemove = vi.fn();

vi.mock('$lib/api/client.svelte', () => ({
  attachments: {
    list: (...a: unknown[]) => attachmentsList(...a),
    upload: (...a: unknown[]) => attachmentsUpload(...a),
    remove: (...a: unknown[]) => attachmentsRemove(...a),
  },
}));

vi.mock('$lib/i18n', () => ({
  createT: () => (key: string, fallbackOrParams?: string | Record<string, unknown>) =>
    typeof fallbackOrParams === 'string' ? fallbackOrParams : key,
  formatDate: (iso: string) => `fmt(${iso})`,
}));

import AttachmentPanel from './AttachmentPanel.svelte';
import type { Attachment } from '$lib/api/attachments';

function makeAttachment(overrides: Partial<Attachment> = {}): Attachment {
  return {
    id: 1,
    name: 'report.pdf',
    size: 2048,
    browser_download_url: '/files/report.pdf',
    ...overrides,
  } as Attachment;
}

function renderPanel(overrides: Record<string, unknown> = {}) {
  // `target` collides with testing-library's mount option — nest props.
  render(AttachmentPanel, {
    props: {
      owner: 'alice',
      repo: 'demo',
      target: 'issues',
      targetId: 5,
      ...overrides,
    },
  });
}

describe('AttachmentPanel.svelte', () => {
  beforeEach(() => {
    attachmentsList.mockReset().mockResolvedValue([]);
    attachmentsUpload.mockReset().mockResolvedValue(undefined);
    attachmentsRemove.mockReset().mockResolvedValue(undefined);
  });

  it('loads attachments scoped to the target on mount', async () => {
    attachmentsList.mockResolvedValue([makeAttachment()]);
    renderPanel();

    await waitFor(() => expect(attachmentsList).toHaveBeenCalledWith('alice', 'demo', 'issues', 5));
    await waitFor(() => expect(screen.getByText('report.pdf')).toBeInTheDocument());
    expect(screen.getByText('report.pdf')).toHaveAttribute('href', '/files/report.pdf');
    expect(screen.getByText('2.0 KiB')).toBeInTheDocument();
  });

  it('renders no list when there are no attachments', async () => {
    renderPanel();

    await waitFor(() => expect(attachmentsList).toHaveBeenCalledTimes(1));
    expect(document.querySelector('.attachment-panel ul')).not.toBeInTheDocument();
  });

  it('uploads the picked file and appends it to the list', async () => {
    attachmentsUpload.mockResolvedValue(makeAttachment({ id: 9, name: 'shot.png', size: 512 }));
    renderPanel();
    await waitFor(() => expect(attachmentsList).toHaveBeenCalledTimes(1));

    const file = new File(['x'], 'shot.png', { type: 'image/png' });
    const input = document.querySelector('input[type="file"]') as HTMLInputElement;
    Object.defineProperty(input, 'files', { value: [file], configurable: true });
    await fireEvent.change(input);

    await waitFor(() =>
      expect(attachmentsUpload).toHaveBeenCalledWith('alice', 'demo', 'issues', 5, file)
    );
    await waitFor(() => expect(screen.getByText('shot.png')).toBeInTheDocument());
    expect(screen.getByText('512 B')).toBeInTheDocument();
  });

  it('removes an attachment from the list after a successful delete', async () => {
    attachmentsList.mockResolvedValue([makeAttachment()]);
    renderPanel();
    await waitFor(() => expect(screen.getByText('report.pdf')).toBeInTheDocument());

    await fireEvent.click(screen.getByText('attachments.delete'));
    await waitFor(() =>
      expect(attachmentsRemove).toHaveBeenCalledWith('alice', 'demo', 'issues', 5, 1)
    );
    await waitFor(() => expect(screen.queryByText('report.pdf')).not.toBeInTheDocument());
  });

  it('surfaces list failures in the error banner', async () => {
    attachmentsList.mockRejectedValue(new Error('storage down'));
    renderPanel();

    await waitFor(() => expect(screen.getByText('storage down')).toBeInTheDocument());
  });
});
