import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@testing-library/svelte';

vi.mock('$lib/i18n', () => ({
  createT: () => (key: string, fallbackOrParams?: string | Record<string, unknown>) =>
    typeof fallbackOrParams === 'string' ? fallbackOrParams : key,
}));

const packagesPublish = vi.fn();
vi.mock('$lib/api/client.svelte', () => ({
  packages: { publish: (...a: unknown[]) => packagesPublish(...a) },
}));

import PackageUploadForm from './PackageUploadForm.svelte';
import { PACKAGE_FORMATS } from '$lib/packageFormats';

function makeFile(name = 'pkg-1.0.0.crate'): File {
  return new File(['bytes'], name, { type: 'application/octet-stream' });
}

describe('PackageUploadForm.svelte', () => {
  beforeEach(() => {
    packagesPublish.mockReset();
  });

  it('renders the format picker with all supported formats', () => {
    render(PackageUploadForm, { owner: 'alice', repo: 'demo', onUploaded: () => {} });

    const select = document.querySelector('select');
    expect(select?.querySelectorAll('option')).toHaveLength(PACKAGE_FORMATS.length);
    // Upload stays disabled until a file is chosen.
    expect(
      (document.querySelector('button[type="submit"]') as HTMLButtonElement).disabled
    ).toBe(true);
  });

  it('requires a file before uploading', async () => {
    render(PackageUploadForm, { owner: 'alice', repo: 'demo', onUploaded: () => {} });

    // Bypass the disabled button by submitting the form directly.
    await fireEvent.submit(document.querySelector('form')!);
    expect(await screen.findByText('Package file is required')).toBeInTheDocument();
    expect(packagesPublish).not.toHaveBeenCalled();
  });

  it('publishes with trimmed metadata and shows the success banner', async () => {
    packagesPublish.mockResolvedValue(undefined);
    const onUploaded = vi.fn();
    render(PackageUploadForm, { owner: 'alice', repo: 'demo', onUploaded });

    const fileInput = document.querySelector<HTMLInputElement>('input[type="file"]');
    Object.defineProperty(fileInput, 'files', { value: [makeFile()] });
    await fireEvent.change(fileInput!);
    // The chosen file name replaces the placeholder label.
    expect(screen.getByText('pkg-1.0.0.crate')).toBeInTheDocument();
    expect(
      (document.querySelector('button[type="submit"]') as HTMLButtonElement).disabled
    ).toBe(false);

    await fireEvent.input(screen.getByLabelText('Name'), { target: { value: '  mypkg ' } });
    await fireEvent.input(screen.getByLabelText('packages.version'), {
      target: { value: ' 1.0.0 ' },
    });
    await fireEvent.submit(document.querySelector('form')!);

    await waitFor(() =>
      expect(packagesPublish).toHaveBeenCalledWith(
        'alice',
        'demo',
        'cargo',
        expect.any(File),
        expect.objectContaining({ name: 'mypkg', version: '1.0.0' })
      )
    );
    await waitFor(() => expect(onUploaded).toHaveBeenCalledTimes(1));
    expect(await screen.findByText('packages.upload_success')).toBeInTheDocument();
  });

  it('shows an inline error when the publish fails', async () => {
    packagesPublish.mockRejectedValue(new Error('bad manifest'));
    render(PackageUploadForm, { owner: 'alice', repo: 'demo', onUploaded: () => {} });

    const fileInput = document.querySelector<HTMLInputElement>('input[type="file"]');
    Object.defineProperty(fileInput, 'files', { value: [makeFile()] });
    await fireEvent.change(fileInput!);
    await fireEvent.submit(document.querySelector('form')!);

    expect(await screen.findByText('bad manifest')).toBeInTheDocument();
  });
});
