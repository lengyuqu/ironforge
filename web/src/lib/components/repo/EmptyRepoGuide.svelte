<script lang="ts">
  // Empty-repo setup guidance — shown when a repository has no commits.
  // Self-contained: computes the HTTP/SSH clone URLs and manages copy state.
  import { browser } from '$app/environment';
  import { buildSshCloneUrl, withBackendBase } from '$lib/api/_base.svelte';
  import { createT } from '$lib/i18n';

  interface Props {
    owner: string;
    repo: string;
    defaultBranch?: string;
  }

  let { owner, repo, defaultBranch = 'main' }: Props = $props();

  const t = createT();

  const httpCloneUrl = $derived(
    withBackendBase(`/git/${encodeURIComponent(owner)}/${encodeURIComponent(repo)}`)
  );
  const sshCloneUrl = $derived(browser ? buildSshCloneUrl(owner, repo, location.hostname) : '');

  let httpCopied = $state(false);
  let sshCopied = $state(false);

  function copyHttp() {
    navigator.clipboard.writeText(httpCloneUrl);
    httpCopied = true;
    setTimeout(() => (httpCopied = false), 2000);
  }

  function copySsh() {
    navigator.clipboard.writeText(sshCloneUrl);
    sshCopied = true;
    setTimeout(() => (sshCopied = false), 2000);
  }
</script>

<div class="empty-repo">
  <div class="empty-icon">📦</div>
  <h2>{t('repo.empty.title')}</h2>
  <p>{t('repo.empty.desc')}</p>

  <div class="setup-steps">
    <div class="step">
      <span class="step-num">1</span>
      <span>{t('repo.empty.step_clone')}</span>
    </div>

    <div class="clone-options">
      <div class="option-box">
        <div class="option-header">
          <strong>HTTPS</strong>
          <button class="mini-copy" onclick={copyHttp}>
            {httpCopied ? '✓ ' + t('repo.empty.copied') : '📋 ' + t('repo.empty.copy')}
          </button>
        </div>
        <code class="cmd">{httpCloneUrl}</code>
      </div>
      <div class="option-box">
        <div class="option-header">
          <strong>SSH</strong>
          <button class="mini-copy" onclick={copySsh}>
            {sshCopied ? '✓ ' + t('repo.empty.copied') : '📋 ' + t('repo.empty.copy')}
          </button>
        </div>
        <code class="cmd">{sshCloneUrl}</code>
      </div>
    </div>

    <div class="step">
      <span class="step-num">2</span>
      <span>{t('repo.empty.step_create')}</span>
    </div>

    <div class="step">
      <span class="step-num">3</span>
      <span>{t('repo.empty.step_push')}</span>
    </div>
  </div>

  <div class="quick-commands">
    <h3>{t('repo.empty.quick')}</h3>
    <pre><code>git init
git add README.md
git commit -m "first commit"
git branch -M {defaultBranch}
git remote add origin {httpCloneUrl}
git push -u origin {defaultBranch}</code></pre>
  </div>

  <div class="or-push">
    <h3>{t('repo.empty.existing')}</h3>
    <pre><code>git remote add origin {httpCloneUrl}
git branch -M {defaultBranch}
git push -u origin {defaultBranch}</code></pre>
  </div>
</div>

<style>
  .empty-repo {
    text-align: center;
    padding: 48px 24px;
  }

  .empty-icon {
    font-size: 48px;
    margin-bottom: 16px;
  }

  .empty-repo h2 {
    font-size: 22px;
    margin-bottom: 8px;
  }
  .empty-repo > p {
    color: var(--text-secondary);
    margin-bottom: 32px;
  }

  .setup-steps {
    max-width: 640px;
    margin: 0 auto 32px;
    text-align: left;
  }

  .step {
    display: flex;
    align-items: flex-start;
    gap: 12px;
    margin-bottom: 12px;
    font-size: 14px;
  }

  .step-num {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 24px;
    height: 24px;
    border-radius: 50%;
    background: var(--accent);
    color: #fff;
    font-size: 12px;
    font-weight: 700;
    flex-shrink: 0;
  }

  .clone-options {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 12px;
    margin: 0 0 24px 36px;
  }

  @media (max-width: 600px) {
    .clone-options {
      grid-template-columns: 1fr;
    }
  }

  .option-box {
    background: var(--bg-secondary);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: 12px;
  }

  .option-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    margin-bottom: 8px;
    font-size: 12px;
  }

  .mini-copy {
    padding: 2px 8px;
    font-size: 11px;
    background: none;
    border: 1px solid var(--border);
    border-radius: 4px;
    cursor: pointer;
    color: var(--text-secondary);
  }
  .mini-copy:hover { background: var(--bg-hover); }

  .cmd {
    font-size: 12px;
    padding: 8px;
    background: var(--bg-primary);
    border: 1px solid var(--border-light);
    border-radius: 4px;
    display: block;
    word-break: break-all;
    user-select: all;
  }

  .quick-commands, .or-push {
    max-width: 640px;
    margin: 0 auto 24px;
    text-align: left;
  }

  .quick-commands h3, .or-push h3 {
    font-size: 14px;
    margin-bottom: 8px;
  }

  .quick-commands pre, .or-push pre {
    background: var(--bg-secondary);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: 16px;
    overflow-x: auto;
  }

  .quick-commands code, .or-push code {
    font-size: 13px;
    line-height: 1.6;
    color: var(--text-primary);
  }
</style>
