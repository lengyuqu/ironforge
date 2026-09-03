import { describe, it, expect } from 'vitest';
import { render, screen } from '@testing-library/svelte';

import CommitInfoCard from './CommitInfoCard.svelte';
import type { RepoCommitEntry } from '$lib/types/entities';

const futureDate = new Date(Date.now() + 3600 * 1000).toISOString();

function makeCommit(overrides: Partial<RepoCommitEntry> = {}): RepoCommitEntry {
  return {
    sha: '0123456789abcdef0123456789abcdef01234567',
    message: 'Add the feature',
    author: 'Alice Dev',
    date: futureDate,
    ...overrides,
  } as RepoCommitEntry;
}

describe('CommitInfoCard.svelte', () => {
  it('renders the commit title, short sha and author', () => {
    render(CommitInfoCard, { commit: makeCommit() });

    expect(screen.getByText('Add the feature')).toBeInTheDocument();
    // getShortSha returns the first 8 chars of the sha.
    expect(screen.getByText('01234567')).toBeInTheDocument();
    expect(screen.getByText('Alice Dev')).toBeInTheDocument();
    expect(screen.getByText('just now')).toBeInTheDocument();
  });

  it('renders the verified GPG badge with the signer name', () => {
    render(CommitInfoCard, {
      commit: makeCommit(),
      gpgSignature: {
        verified: true,
        signer_key: 'KEY',
        signer_name: 'Alice Dev',
        signer_email: 'alice@example.com',
        status: 'good',
      },
    });

    expect(screen.getByText('Signed')).toBeInTheDocument();
    expect(screen.getByText((c) => c.includes('by Alice Dev'))).toBeInTheDocument();
  });

  it('renders the unsigned badge when there is no signature', () => {
    render(CommitInfoCard, {
      commit: makeCommit(),
      gpgSignature: {
        verified: false,
        signer_key: null,
        signer_name: null,
        signer_email: null,
        status: 'no_signature',
      },
    });

    expect(screen.getByText('Unsigned')).toBeInTheDocument();
  });

  it('renders the bad-signature badge for an unverified status', () => {
    render(CommitInfoCard, {
      commit: makeCommit(),
      gpgSignature: {
        verified: false,
        signer_key: 'KEY',
        signer_name: null,
        signer_email: null,
        status: 'bad',
      },
    });

    expect(screen.getByText('Bad signature')).toBeInTheDocument();
  });
});
