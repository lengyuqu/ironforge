<script lang="ts">
  // Commit info card — pure presentation: title, short sha, author/date and
  // the GPG signature badge.
  import type { RepoCommitEntry } from '$lib/types/entities';

  interface GpgSignature {
    verified: boolean;
    signer_key: string | null;
    signer_name: string | null;
    signer_email: string | null;
    status: string;
  }

  interface Props {
    commit: RepoCommitEntry;
    gpgSignature?: GpgSignature | null;
  }

  let { commit, gpgSignature = null }: Props = $props();

  function getShortSha(fullSha: string): string {
    return fullSha.substring(0, 8);
  }

  function formatDate(dateStr: string): string {
    const date = new Date(dateStr);
    const now = new Date();
    const diffMs = now.getTime() - date.getTime();
    const diffMins = Math.floor(diffMs / 60000);
    const diffHours = Math.floor(diffMins / 60);
    const diffDays = Math.floor(diffHours / 24);

    if (diffMins < 1) return 'just now';
    if (diffMins < 60) return `${diffMins} minutes ago`;
    if (diffHours < 24) return `${diffHours} hours ago`;
    if (diffDays < 7) return `${diffDays} days ago`;

    return date.toLocaleDateString('en-US', {
      year: 'numeric',
      month: 'short',
      day: 'numeric'
    });
  }
</script>

<div class="commit-info">
  <div class="commit-header">
    <h1 class="commit-title">{commit.message}</h1>
    <div class="commit-sha">
      <code>{getShortSha(commit.sha)}</code>
    </div>
  </div>
  <div class="commit-meta">
    <span class="commit-author">{commit.author}</span>
    <span class="commit-date">{formatDate(commit.date)}</span>
    {#if gpgSignature}
      <span
        class="gpg-badge"
        class:verified={gpgSignature.verified}
        class:unverified={!gpgSignature.verified && gpgSignature.status !== 'no_signature'}
        class:no-sig={gpgSignature.status === 'no_signature'}
        title="GPG: {gpgSignature.status}{gpgSignature.signer_name ? ' · ' + gpgSignature.signer_name : ''}"
      >
        {#if gpgSignature.verified}
          <span class="gpg-icon">✓</span> Signed
          {#if gpgSignature.signer_name}
            <span class="gpg-signer">by {gpgSignature.signer_name}</span>
          {/if}
        {:else if gpgSignature.status === 'no_signature'}
          <span class="gpg-icon">○</span> Unsigned
        {:else}
          <span class="gpg-icon">✗</span> Bad signature
        {/if}
      </span>
    {/if}
  </div>
</div>

<style>
  .commit-info {
    background: var(--bg-secondary);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: 1.5rem;
    margin-bottom: 1.5rem;
  }

  .commit-header {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: 1rem;
    margin-bottom: 1rem;
  }

  .commit-title {
    font-size: 1.5rem;
    font-weight: 600;
    margin: 0;
    color: var(--text-primary);
    flex: 1;
  }

  .commit-sha code {
    font-family: 'Monaco', 'Menlo', 'Ubuntu Mono', monospace;
    font-size: 0.875rem;
    background: var(--bg-primary);
    padding: 0.25rem 0.5rem;
    border-radius: var(--radius);
    color: var(--accent);
    border: 1px solid var(--border);
  }

  .commit-meta {
    display: flex;
    gap: 1rem;
    font-size: 0.875rem;
    color: var(--text-secondary);
  }

  .commit-author {
    font-weight: 500;
  }

  .gpg-badge {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    padding: 3px 10px;
    border-radius: 12px;
    font-size: 11px;
    font-weight: 600;
    cursor: default;
    white-space: nowrap;
  }
  .gpg-badge.verified {
    background: rgba(63, 185, 80, 0.12);
    color: var(--green, #3fb950);
    border: 1px solid rgba(63, 185, 80, 0.25);
  }
  .gpg-badge.unverified {
    background: rgba(248, 81, 73, 0.1);
    color: var(--red, #f85149);
    border: 1px solid rgba(248, 81, 73, 0.2);
  }
  .gpg-badge.no-sig {
    background: var(--bg-tertiary);
    color: var(--text-muted);
    border: 1px solid var(--border-light);
  }
  .gpg-icon { font-size: 12px; }
  .gpg-signer { font-weight: 400; opacity: 0.85; }

  @media (max-width: 900px) {
    .commit-header {
      flex-direction: column;
      gap: 0.5rem;
    }

    .commit-title {
      font-size: 1.25rem;
    }
  }
</style>
