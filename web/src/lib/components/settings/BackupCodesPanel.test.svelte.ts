import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/svelte';

const toastSuccess = vi.fn();
const toastError = vi.fn();
vi.mock('$lib/components/toast.svelte', () => ({
  toast: { success: (...a: unknown[]) => toastSuccess(...a), error: (...a: unknown[]) => toastError(...a) },
}));

import BackupCodesPanel from './BackupCodesPanel.svelte';

const writeText = vi.fn();

describe('BackupCodesPanel.svelte', () => {
  beforeEach(() => {
    writeText.mockReset().mockResolvedValue(undefined);
    toastSuccess.mockClear();
    toastError.mockClear();
    vi.stubGlobal('navigator', { clipboard: { writeText } });
  });
  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it('renders one code element per backup code', () => {
    render(BackupCodesPanel, { codes: ['AAAA-BBBB', 'CCCC-DDDD', 'EEEE-FFFF'] });

    expect(screen.getByText('AAAA-BBBB')).toBeInTheDocument();
    expect(screen.getByText('CCCC-DDDD')).toBeInTheDocument();
    expect(screen.getByText('EEEE-FFFF')).toBeInTheDocument();
    expect(document.querySelectorAll('.code-grid code').length).toBe(3);
  });

  it('copies all codes newline-joined and reports success', async () => {
    render(BackupCodesPanel, { codes: ['AAAA-BBBB', 'CCCC-DDDD'] });

    await fireEvent.click(screen.getByText('Copy Codes'));
    expect(writeText).toHaveBeenCalledWith('AAAA-BBBB\nCCCC-DDDD');
    expect(toastSuccess).toHaveBeenCalledWith('Backup codes copied');
    expect(toastError).not.toHaveBeenCalled();
  });

  it('reports an error toast when the clipboard write fails', async () => {
    writeText.mockRejectedValueOnce(new Error('denied'));
    render(BackupCodesPanel, { codes: ['AAAA-BBBB'] });

    await fireEvent.click(screen.getByText('Copy Codes'));
    expect(toastError).toHaveBeenCalledWith('Copy failed');
    expect(toastSuccess).not.toHaveBeenCalled();
  });
});
