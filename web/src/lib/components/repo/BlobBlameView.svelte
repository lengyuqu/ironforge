<script lang="ts">
  // Blame view — per-line authorship for a file. Self-contained: loads the
  // blame for the given ref/path and renders one row per line, with the
  // introducing commit shown on the first row of each hunk.
  import EmptyState from '$lib/components/EmptyState.svelte';
  import { repos } from '$lib/api/client.svelte';
  import { createT, formatDate } from '$lib/i18n';
  import { toErrorMessage } from '$lib/utils/error';
  import { buildCommitHref } from '$lib/utils/repoUrls';
  import type { BlameResult } from '$lib/types/entities';

  interface Props {
    owner: string;
    repo: string;
    ref: string;
    filePath: string;
  }

  let { owner, repo, ref, filePath }: Props = $props();

  const t = createT();

  let result = $state<BlameResult | null>(null);
  let loading = $state(true);
  let error = $state('');
  // Guards against a slow earlier response overwriting a newer one.
  let requestId = 0;

  $effect(() => {
    load(owner, repo, ref, filePath);
  });

  async function load(o: string, r: string, activeRef: string, path: string) {
    const id = ++requestId;
    loading = true;
    error = '';
    try {
      const data = await repos.blame(o, r, path, activeRef || undefined);
      if (id === requestId) result = data;
    } catch (e: unknown) {
      if (id === requestId) {
        result = null;
        error = t('repo.blob.blame_failed', { message: toErrorMessage(e, '') });
      }
    } finally {
      if (id === requestId) loading = false;
    }
  }
</script>

<div class="blame-view">
  {#if loading}
    <EmptyState title={t('repo.blob.blame_loading')} />
  {:else if error}
    <div class="empty-state error-text">{error}</div>
  {:else if result}
    <table class="blame-table">
      <tbody>
        {#each result.hunks as hunk (hunk.start_line)}
          {#each hunk.lines as line, i (i)}
            <tr class:hunk-start={i === 0}>
              <td class="blame-meta">
                {#if i === 0}
                  <a
                    class="blame-sha"
                    href={buildCommitHref(owner, repo, ref, hunk.commit.sha)}
                    title={hunk.original_path
                      ? t('repo.blob.blame_renamed_from', { path: hunk.original_path })
                      : hunk.commit.message}
                  >
                    {hunk.commit.sha.slice(0, 7)}
                  </a>
                  <span class="blame-message">{hunk.commit.message}</span>
                  <span class="blame-author">{hunk.commit.author}</span>
                  <span class="blame-date">{formatDate(hunk.commit.date)}</span>
                {/if}
              </td>
              <td class="line-number">{hunk.start_line + i}</td>
              <td class="line-content"><code>{line || ' '}</code></td>
            </tr>
          {/each}
        {/each}
      </tbody>
    </table>
  {/if}
</div>

<style>
  .blame-view {
    background: var(--bg-secondary);
    border: 1px solid var(--border);
    border-radius: 0 0 var(--radius) var(--radius);
    overflow: auto;
  }

  .blame-table {
    width: 100%;
    border-collapse: collapse;
    font-size: 13px;
    line-height: 1.6;
  }

  .hunk-start td {
    border-top: 1px solid var(--border-light);
  }

  .hunk-start:first-child td {
    border-top: none;
  }

  .blame-meta {
    display: flex;
    align-items: baseline;
    gap: 8px;
    min-width: 280px;
    max-width: 420px;
    padding: 0 12px 0 16px;
    font-size: 12px;
    color: var(--text-muted);
    white-space: nowrap;
    overflow: hidden;
  }

  .blame-sha {
    font-family: ui-monospace, SFMono-Regular, 'SF Mono', Menlo, Consolas, monospace;
    flex-shrink: 0;
  }

  .blame-message {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    color: var(--text-secondary);
  }

  .blame-author,
  .blame-date {
    flex-shrink: 0;
  }

  .line-number {
    padding: 0 12px;
    text-align: right;
    color: var(--text-muted);
    font-size: 12px;
    user-select: none;
    border-left: 1px solid var(--border-light);
    border-right: 1px solid var(--border-light);
    vertical-align: top;
    width: 1%;
    white-space: nowrap;
  }

  .line-content {
    padding: 0 16px;
    white-space: pre;
    font-family: ui-monospace, SFMono-Regular, 'SF Mono', Menlo, Consolas, monospace;
  }

  .line-content code {
    font-family: inherit;
    tab-size: 4;
  }

  .empty-state {
    padding: 24px;
    color: var(--text-muted);
  }

  .error-text {
    color: var(--red);
  }

  @media (max-width: 820px) {
    .blame-meta {
      min-width: 160px;
    }
    .blame-message,
    .blame-date {
      display: none;
    }
  }
</style>
