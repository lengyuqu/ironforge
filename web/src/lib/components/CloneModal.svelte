<script lang="ts">
  import { onMount } from 'svelte';
  import { buildSshCloneUrl, downloadApiFile, withBackendBase } from '$lib/api/_base.svelte';
  import { browser } from '$app/environment';
  import { createT } from '$lib/i18n';
  import { toast } from './toast.svelte';

  interface Props {
    owner: string;
    repo: string;
    archiveRef?: string;
    open: boolean;
    onClose: () => void;
  }

  let { owner, repo, archiveRef = 'main', open, onClose }: Props = $props();

  const t = createT();

  let cloneTab = $state<'http' | 'ssh'>('http');
  let httpCopied = $state(false);
  let sshCopied = $state(false);
  let downloadingArchive = $state(false);

  let httpCloneUrl = $derived(withBackendBase(`/git/${encodeURIComponent(owner)}/${encodeURIComponent(repo)}`));
  let sshCloneUrl = $derived(browser ? buildSshCloneUrl(owner, repo, location.hostname) : '');

  function copyUrl(url: string) {
    navigator.clipboard.writeText(url);
    if (cloneTab === 'http') {
      httpCopied = true;
      setTimeout(() => (httpCopied = false), 2000);
    } else {
      sshCopied = true;
      setTimeout(() => (sshCopied = false), 2000);
    }
  }

  async function downloadArchive() {
    try {
      downloadingArchive = true;
      await downloadApiFile(
        `/repos/${encodeURIComponent(owner)}/${encodeURIComponent(repo)}/archive/${encodeURIComponent(archiveRef)}.zip`,
        `${repo}-${archiveRef || 'archive'}.zip`
      );
      toast.success(t('repo.download_started', 'Archive download started'));
    } catch (e: unknown) {
      const msg = e instanceof Error ? e.message : String(e);
      toast.error(t('repo.download_failed', 'Download failed') + ': ' + msg);
    } finally {
      downloadingArchive = false;
    }
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === 'Escape' && open) {
      e.preventDefault();
      onClose();
    }
  }

  onMount(() => {
    window.addEventListener('keydown', handleKeydown);
    return () => window.removeEventListener('keydown', handleKeydown);
  });
</script>

{#if open}
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div class="clone-backdrop" onclick={onClose} role="presentation" tabindex="-1"></div>
  <div
    id="clone-dropdown"
    class="clone-dropdown"
    role="dialog"
    aria-label={t('repo.clone_title')}
    tabindex="-1"
  >
    <div class="clone-header">
      <span class="clone-title">{t('repo.clone_title')}</span>
    </div>

    <!-- Tab bar -->
    <div class="clone-tabs" role="tablist">
      <button
        class="clone-tab"
        class:active={cloneTab === 'http'}
        onclick={() => (cloneTab = 'http')}
        role="tab"
        aria-selected={cloneTab === 'http'}
        aria-label="HTTPS clone"
      >HTTPS</button>
      <button
        class="clone-tab"
        class:active={cloneTab === 'ssh'}
        onclick={() => (cloneTab = 'ssh')}
        role="tab"
        aria-selected={cloneTab === 'ssh'}
        aria-label="SSH clone"
      >SSH</button>
    </div>

    <!-- URL input with copy -->
    <div class="clone-input-row">
      <input
        type="text"
        class="clone-input"
        readonly
        value={cloneTab === 'http' ? httpCloneUrl : sshCloneUrl}
        aria-label={t('repo.clone_url')}
      />
      <button
        class="clone-copy-btn"
        onclick={() => copyUrl(cloneTab === 'http' ? httpCloneUrl : sshCloneUrl)}
        aria-label={t('repo.copy_clone_url')}
        title={t('repo.copy_clone_url')}
      >
        {#if (cloneTab === 'http' && httpCopied) || (cloneTab === 'ssh' && sshCopied)}
          <span class="copied-check" aria-hidden="true">✓</span>
        {:else}
          <svg width="16" height="16" viewBox="0 0 16 16" fill="currentColor" aria-hidden="true">
            <path fill-rule="evenodd" d="M0 6.75C0 5.784.784 5 1.75 5h1.5a.75.75 0 010 1.5h-1.5a.25.25 0 00-.25.25v7.5c0 .138.112.25.25.25h7.5a.25.25 0 00.25-.25v-1.5a.75.75 0 011.5 0v1.5A1.75 1.75 0 019.25 16h-7.5A1.75 1.75 0 010 14.25v-7.5z"></path>
            <path fill-rule="evenodd" d="M5 1.75C5 .784 5.784 0 6.75 0h7.5C15.216 0 16 .784 16 1.75v7.5A1.75 1.75 0 0114.25 11h-7.5A1.75 1.75 0 015 9.25v-7.5zm1.75-.25a.25.25 0 00-.25.25v7.5c0 .138.112.25.25.25h7.5a.25.25 0 00.25-.25v-7.5a.25.25 0 00-.25-.25h-7.5z"></path>
          </svg>
        {/if}
      </button>
    </div>

    <!-- Download ZIP -->
    <div class="clone-footer">
      <button
        type="button"
        class="clone-footer-link"
        aria-label={t('repo.download_zip')}
        onclick={downloadArchive}
        disabled={downloadingArchive}
      >
        <svg width="16" height="16" viewBox="0 0 16 16" fill="currentColor" aria-hidden="true">
          <path fill-rule="evenodd" d="M2.75 14A1.75 1.75 0 011 12.25v-2.5a.75.75 0 011.5 0v2.5c0 .138.112.25.25.25h10.5a.25.25 0 00.25-.25v-2.5a.75.75 0 011.5 0v2.5A1.75 1.75 0 0113.25 14H2.75z"></path>
          <path fill-rule="evenodd" d="M7.25 1.75A.75.75 0 018.75 1v6.69l1.47-1.47a.75.75 0 111.06 1.06l-2.75 2.75a.75.75 0 01-1.06 0L4.72 7.28a.75.75 0 011.06-1.06l1.47 1.47V1.75z"></path>
        </svg>
        {t('repo.download_zip')}
      </button>
    </div>
  </div>
{/if}

<style>
  .clone-backdrop {
    position: fixed;
    inset: 0;
    z-index: 299;
  }

  .clone-dropdown {
    position: absolute;
    top: 100%;
    right: 0;
    margin-top: 4px;
    background: var(--bg-primary);
    border: 1px solid var(--border);
    border-radius: 6px;
    width: 340px;
    box-shadow: 0 8px 24px rgba(0,0,0,0.15);
    z-index: 300;
  }

  .clone-header {
    padding: 8px 12px;
    border-bottom: 1px solid var(--border);
  }

  .clone-title {
    font-size: 12px;
    font-weight: 600;
    color: var(--text-primary);
  }

  .clone-tabs {
    display: flex;
    border-bottom: 1px solid var(--border);
  }

  .clone-tab {
    flex: 1;
    padding: 8px 12px;
    background: none;
    border: none;
    border-bottom: 2px solid transparent;
    font-size: 12px;
    font-weight: 500;
    color: var(--text-secondary);
    cursor: pointer;
  }
  .clone-tab:hover {
    color: var(--text-primary);
    background: var(--bg-secondary);
  }
  .clone-tab.active {
    color: var(--text-primary);
    font-weight: 600;
    border-bottom-color: var(--accent);
  }

  .clone-input-row {
    display: flex;
    padding: 8px 12px;
    gap: 0;
  }

  .clone-input {
    flex: 1;
    padding: 5px 8px;
    font-size: 12px;
    font-family: ui-monospace, SFMono-Regular, "SF Mono", Menlo, Consolas, monospace;
    line-height: 20px;
    border: 1px solid var(--border);
    border-right: none;
    border-radius: 6px 0 0 6px;
    background: var(--bg-secondary);
    color: var(--text-primary);
    outline: none;
  }

  .clone-copy-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 5px 8px;
    background: var(--bg-secondary);
    border: 1px solid var(--border);
    border-left: none;
    border-radius: 0 6px 6px 0;
    cursor: pointer;
    color: var(--text-secondary);
    min-width: 32px;
  }
  .clone-copy-btn:hover {
    background: var(--bg-hover);
    color: var(--text-primary);
  }

  .copied-check {
    color: var(--green);
    font-size: 14px;
    font-weight: 700;
  }

  .clone-footer {
    padding: 8px 12px;
    border-top: 1px solid var(--border);
  }

  .clone-footer-link {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 0;
    border: 0;
    background: none;
    font-size: 12px;
    color: var(--accent);
    text-decoration: none;
    cursor: pointer;
  }
  .clone-footer-link:hover {
    text-decoration: underline;
  }
  .clone-footer-link:disabled {
    cursor: wait;
    opacity: 0.65;
  }
</style>
