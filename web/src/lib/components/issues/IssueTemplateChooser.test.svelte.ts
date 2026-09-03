import { describe, it, expect, vi } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/svelte';

vi.mock('$lib/i18n', () => ({
  createT: () => (key: string, fallbackOrParams?: string | Record<string, unknown>) =>
    typeof fallbackOrParams === 'string' ? fallbackOrParams : key,
}));

import IssueTemplateChooser from './IssueTemplateChooser.svelte';
import type { IssueTemplate, IssueConfig } from '$lib/api/client.svelte';

const template: IssueTemplate = {
  name: 'Bug report',
  title: 'Bug',
  about: 'Report a bug',
  labels: ['bug'],
  assignees: [],
  ref: 'bug',
  content: 'body',
  file_name: 'bug.md',
};

const config: IssueConfig = {
  blank_issues_enabled: true,
  contact_links: [{ name: 'Contact', url: 'https://example.com/contact', about: 'Reach us' }],
};

describe('IssueTemplateChooser.svelte', () => {
  it('chooses a template when its get-started button is clicked', async () => {
    const onChoose = vi.fn();
    render(IssueTemplateChooser, { templates: [template], config, warning: '', onChoose, onClose: vi.fn() });

    await fireEvent.click(screen.getByText('issues.templates.get_started'));
    expect(onChoose).toHaveBeenCalledWith(template);
  });

  it('opens a blank issue with no argument when blank is enabled', async () => {
    const onChoose = vi.fn();
    render(IssueTemplateChooser, { templates: [], config, warning: '', onChoose, onClose: vi.fn() });

    await fireEvent.click(screen.getByText('issues.templates.open_blank'));
    expect(onChoose).toHaveBeenCalledTimes(1);
    expect((onChoose as ReturnType<typeof vi.fn>).mock.calls[0][0]).toBeUndefined();
  });

  it('renders contact links as external anchors', () => {
    const onChoose = vi.fn();
    render(IssueTemplateChooser, { templates: [], config, warning: '', onChoose, onClose: vi.fn() });

    const link = screen.getByText('issues.templates.open_link').closest('a') as HTMLAnchorElement;
    expect(link.href).toBe('https://example.com/contact');
    expect(link.target).toBe('_blank');
  });

  it('shows the config warning when the warning prop is set', () => {
    const onChoose = vi.fn();
    render(IssueTemplateChooser, {
      templates: [],
      config,
      warning: 'bad yaml',
      onChoose,
      onClose: vi.fn(),
    });
    expect(screen.getByText(/bad yaml/)).toBeInTheDocument();
  });

  it('closes the chooser via the cancel button', async () => {
    const onClose = vi.fn();
    render(IssueTemplateChooser, { templates: [], config, warning: '', onChoose: vi.fn(), onClose });

    await fireEvent.click(screen.getByText('issues.create_form.cancel'));
    expect(onClose).toHaveBeenCalledTimes(1);
  });
});
