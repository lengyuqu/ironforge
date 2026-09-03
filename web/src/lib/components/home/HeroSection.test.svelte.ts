import { describe, it, expect, vi } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/svelte';

const goto = vi.fn();
vi.mock('$app/navigation', () => ({ goto: (...a: unknown[]) => goto(...a) }));

vi.mock('$lib/i18n', () => ({
  createT: () => (key: string, fallbackOrParams?: string | Record<string, unknown>) =>
    typeof fallbackOrParams === 'string' ? fallbackOrParams : key,
  formatDate: (iso: string) => `fmt(${iso})`,
}));

import HeroSection from './HeroSection.svelte';

describe('HeroSection.svelte', () => {
  it('renders the brand title and both i18n tagline texts', () => {
    render(HeroSection);

    expect(screen.getByText('IronForge')).toBeInTheDocument();
    expect(screen.getByText('home.tagline')).toBeInTheDocument();
    expect(screen.getByText('home.description')).toBeInTheDocument();
  });

  it('routes the sign-up and sign-in buttons', () => {
    render(HeroSection);

    fireEvent.click(screen.getByText('home.sign_up'));
    expect(goto).toHaveBeenCalledWith('/register');

    fireEvent.click(screen.getByText('home.sign_in'));
    expect(goto).toHaveBeenCalledWith('/login');
  });
});
