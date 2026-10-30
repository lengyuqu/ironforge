import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@testing-library/svelte';

// The page reads owner/repo from the SvelteKit page store; the alias in
// vitest.config points at the kit runtime re-export, so this mock intercepts it.
const pageStore = vi.hoisted(() => {
  const value = { params: { owner: 'alice', repo: 'demo' } };
  return {
    value,
    page: {
      subscribe(fn: (v: typeof value) => void) {
        fn(value);
        return () => {};
      },
    },
  };
});
vi.mock('$app/stores', () => ({ page: pageStore.page }));

const api = vi.hoisted(() => ({
  list: vi.fn(),
  put: vi.fn(),
  remove: vi.fn(),
}));

vi.mock('$lib/api/client.svelte', () => ({
  ciSecrets: {
    list: (...a: unknown[]) => api.list(...a),
    put: (...a: unknown[]) => api.put(...a),
    delete: (...a: unknown[]) => api.remove(...a),
  },
}));

vi.mock('$lib/components/confirm.svelte', () => ({
  confirmDialog: async () => true,
}));

// Only the keys this page interpolates need real text; everything else falls
// back to the key so the assertions stay about behaviour, not copy.
const hintText = 'Saved as {name}';
const reservedText = '{name} is reserved';
vi.mock('$lib/i18n', () => ({
  createT: () => (key: string, fallbackOrParams?: string | Record<string, unknown>) => {
    if (typeof fallbackOrParams === 'string') return fallbackOrParams;
    const template = key === 'settings.ci_secrets.name_hint' ? hintText
      : key === 'settings.ci_secrets.name_reserved' ? reservedText
      : key;
    if (!fallbackOrParams) return template;
    return template.replace(/\{(\w+)\}/g, (_, name: string) => String(fallbackOrParams[name] ?? `{${name}}`));
  },
  formatDateTime: (iso: string) => `fmt(${iso})`,
}));

import Page from './+page.svelte';

function renderPage() {
  return render(Page);
}

const nameInput = () => screen.getByLabelText('settings.ci_secrets.name') as HTMLInputElement;
const valueInput = () => screen.getByLabelText('settings.ci_secrets.value') as HTMLInputElement;
const saveButton = () => screen.getByText('settings.ci_secrets.save');

async function type(field: HTMLInputElement, text: string) {
  await fireEvent.input(field, { target: { value: text } });
}

describe('ci-secrets settings page', () => {
  beforeEach(() => {
    Object.values(api).forEach((fn) => fn.mockReset());
    api.list.mockResolvedValue([]);
    api.put.mockResolvedValue({ name: 'DEPLOY_TOKEN', created_at: '', updated_at: '' });
    pageStore.value.params.owner = 'alice';
    pageStore.value.params.repo = 'demo';
  });

  it('submits a lowercase name uppercased to the API', async () => {
    renderPage();
    await waitFor(() => expect(api.list).toHaveBeenCalledWith('alice', 'demo'));

    await type(nameInput(), 'deploy_token');
    await type(valueInput(), 's3cret-value');
    await fireEvent.click(saveButton());

    await waitFor(() =>
      expect(api.put).toHaveBeenCalledWith('alice', 'demo', 'DEPLOY_TOKEN', 's3cret-value')
    );
  });

  it('blocks names with characters the backend rejects', async () => {
    renderPage();
    await waitFor(() => expect(api.list).toHaveBeenCalled());

    await type(nameInput(), 'deploy-token');
    await type(valueInput(), 's3cret-value');
    await fireEvent.click(saveButton());

    // Native constraint validation rejects it before save() ever runs.
    expect(nameInput().validity.patternMismatch).toBe(true);
    expect(api.put).not.toHaveBeenCalled();
  });

  it('blocks an empty name', async () => {
    renderPage();
    await waitFor(() => expect(api.list).toHaveBeenCalled());

    await type(nameInput(), '');
    await type(valueInput(), 's3cret-value');
    await fireEvent.click(saveButton());

    expect(nameInput().validity.valueMissing).toBe(true);
    expect(api.put).not.toHaveBeenCalled();
  });

  it('blocks a name starting with a digit or containing a space', async () => {
    renderPage();
    await waitFor(() => expect(api.list).toHaveBeenCalled());

    for (const bad of ['2TOKEN', 'deploy token', '_'.repeat(101)]) {
      await type(nameInput(), bad);
      await type(valueInput(), 's3cret-value');
      await fireEvent.click(saveButton());
      expect(nameInput().validity.patternMismatch || nameInput().validity.tooLong).toBe(true);
    }
    expect(api.put).not.toHaveBeenCalled();
  });

  it('shows what the name will be saved as while it is typed', async () => {
    renderPage();
    await waitFor(() => expect(api.list).toHaveBeenCalled());

    await type(nameInput(), ' deploy_token ');
    expect(screen.getByText('Saved as DEPLOY_TOKEN')).toBeInTheDocument();
    expect(nameInput()).toHaveAccessibleDescription('Saved as DEPLOY_TOKEN');

    // The hint only appears once there is something to normalise.
    await type(nameInput(), '   ');
    expect(screen.queryByText(/Saved as/)).not.toBeInTheDocument();
  });

  it('refuses a reserved name the server would reject, before calling the API', async () => {
    renderPage();
    await waitFor(() => expect(api.list).toHaveBeenCalled());

    await type(nameInput(), 'ci_job_token');
    await type(valueInput(), 's3cret-value');
    await fireEvent.click(saveButton());

    // The shape is valid, so the browser lets it through; the reserved-name
    // check has to catch it instead of letting the API answer 400.
    expect(nameInput().validity.patternMismatch).toBe(false);
    await waitFor(() => expect(api.put).not.toHaveBeenCalled());
    expect(screen.getByRole('alert')).toHaveTextContent('CI_JOB_TOKEN is reserved');
  });
});
