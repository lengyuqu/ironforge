import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@testing-library/svelte';

// Real instance store shape: expose the banner API so save syncing can be
// observed without touching component internals.
const setBanner = vi.fn();
const clearBanner = vi.fn();
vi.mock('$lib/stores/instance.svelte', () => ({
  setBanner: (...a: unknown[]) => setBanner(...a),
  clearBanner: (...a: unknown[]) => clearBanner(...a),
}));

const updateSettings = vi.fn();
vi.mock('$lib/api/client.svelte', () => ({
  admin: { updateSettings: (...a: unknown[]) => updateSettings(...a) },
}));

const toastSuccess = vi.fn();
const toastError = vi.fn();
vi.mock('$lib/components/toast.svelte', () => ({
  toast: {
    success: (...a: unknown[]) => toastSuccess(...a),
    error: (...a: unknown[]) => toastError(...a),
  },
}));

import InstanceSettingsSection from './InstanceSettingsSection.svelte';

describe('InstanceSettingsSection.svelte', () => {
  beforeEach(() => {
    updateSettings.mockReset();
    setBanner.mockClear();
    clearBanner.mockClear();
    toastSuccess.mockClear();
    toastError.mockClear();
  });

  it('renders the initial maintenance mode and banner values', () => {
    render(InstanceSettingsSection, {
      initialMaintenanceMode: true,
      initialBannerMessage: 'hello',
      initialBannerType: 'warning',
    });

    expect(
      (screen.getByLabelText(/Enable maintenance mode/) as HTMLInputElement).checked
    ).toBe(true);
    expect(
      (screen.getByLabelText(/Banner Message/) as HTMLInputElement).value
    ).toBe('hello');
  });

  it('saves settings and syncs the banner store when a message is set', async () => {
    updateSettings.mockResolvedValue({
      maintenance_mode: true,
      banner_message: 'scheduled',
      banner_type: 'info',
    });
    render(InstanceSettingsSection, {
      initialMaintenanceMode: false,
      initialBannerMessage: '',
      initialBannerType: 'info',
    });

    await fireEvent.input(screen.getByLabelText(/Banner Message/), {
      target: { value: 'scheduled' },
    });
    await fireEvent.click(screen.getByText('Save Settings'));

    await waitFor(() =>
      expect(updateSettings).toHaveBeenCalledWith({
        maintenance_mode: false,
        banner_message: 'scheduled',
        banner_type: 'info',
      })
    );
    await waitFor(() => expect(setBanner).toHaveBeenCalledWith('scheduled', 'info'));
    await waitFor(() => expect(clearBanner).not.toHaveBeenCalled());
    await waitFor(() => expect(toastSuccess).toHaveBeenCalledWith('Settings saved'));
  });

  it('clears the banner store when saving an empty message', async () => {
    updateSettings.mockResolvedValue({
      maintenance_mode: false,
      banner_message: null,
      banner_type: 'info',
    });
    render(InstanceSettingsSection, {
      initialMaintenanceMode: false,
      initialBannerMessage: 'old',
      initialBannerType: 'info',
    });

    await fireEvent.input(screen.getByLabelText(/Banner Message/), {
      target: { value: '' },
    });
    await fireEvent.click(screen.getByText('Save Settings'));

    await waitFor(() => expect(clearBanner).toHaveBeenCalledTimes(1));
    expect(setBanner).not.toHaveBeenCalled();
  });

  it('reports failures via toast and stops the saving state', async () => {
    updateSettings.mockRejectedValue(new Error('nope'));
    render(InstanceSettingsSection, {
      initialMaintenanceMode: false,
      initialBannerMessage: '',
      initialBannerType: 'info',
    });

    await fireEvent.click(screen.getByText('Save Settings'));
    await waitFor(() => expect(toastError).toHaveBeenCalledWith('nope'));
    await waitFor(() =>
      expect((screen.getByText('Save Settings') as HTMLButtonElement).disabled).toBe(false)
    );
  });
});
