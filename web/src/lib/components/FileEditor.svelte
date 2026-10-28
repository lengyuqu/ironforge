<script lang="ts">
  import { createT } from '$lib/i18n';
  import { filePathErrorKey, isConflictError, isMarkdownPath } from '$lib/utils/editorFile';
  import CodeEditorPane from '$lib/components/editor/CodeEditorPane.svelte';
  import EditorPreviewPane from '$lib/components/editor/EditorPreviewPane.svelte';
  import EditorDiffPane from '$lib/components/editor/EditorDiffPane.svelte';

  type EditorMode = 'create' | 'edit';
  type EditorTab = 'edit' | 'preview' | 'diff';
  type SavePayload = {
    path: string;
    content: string;
    message: string;
    branch: string;
    sha?: string;
  };

  interface Props {
    owner: string;
    repo: string;
    mode: EditorMode;
    initialPath?: string;
    initialContent?: string;
    initialSha?: string;
    branch?: string;
    cancelHref: string;
    disabledReason?: string;
    onSave: (payload: SavePayload) => Promise<void>;
  }

  const t = createT();

  let {
    owner,
    repo,
    mode,
    initialPath = '',
    initialContent = '',
    initialSha = '',
    branch = 'main',
    cancelHref,
    disabledReason = '',
    onSave,
  }: Props = $props();

  let filePath = $state('');
  let fileContent = $state('');
  let commitMessage = $state('');
  let targetBranch = $state('');
  let activeTab = $state<EditorTab>('edit');
  let saving = $state(false);
  let error = $state('');
  let conflict = $state(false);
  let initialized = $state(false);

  let isMarkdown = $derived(isMarkdownPath(filePath));
  let changed = $derived(mode === 'create' || initialContent !== fileContent);

  $effect(() => {
    if (initialized) return;
    filePath = initialPath;
    fileContent = initialContent;
    commitMessage =
      mode === 'create'
        ? `Create ${initialPath || 'new file'}`
        : `Update ${initialPath || 'file'}`;
    targetBranch = branch;
    initialized = true;
  });

  function validate(): boolean {
    const pathErrorKey = filePathErrorKey(filePath);
    error = pathErrorKey ? t(pathErrorKey) : '';
    if (error) return false;

    if (!commitMessage.trim()) {
      error = t('repo.editor.message_required');
      return false;
    }

    if (!targetBranch.trim()) {
      error = t('repo.editor.branch_required');
      return false;
    }

    if (disabledReason) {
      error = disabledReason;
      return false;
    }

    return true;
  }

  async function saveFile() {
    if (!validate()) return;

    saving = true;
    error = '';
    conflict = false;

    const payload: SavePayload = {
      path: filePath.trim(),
      content: fileContent,
      message: commitMessage.trim(),
      branch: targetBranch.trim(),
      ...(mode === 'edit' && initialSha ? { sha: initialSha } : {}),
    };

    try {
      await onSave(payload);
    } catch (err) {
      const message = err instanceof Error ? err.message : String(err);
      conflict = isConflictError(message);
      error = conflict
        ? t('repo.editor.conflict')
        : t('repo.editor.save_failed', { message });
    } finally {
      saving = false;
    }
  }
</script>

<div class="editor-shell">
  <div class="editor-header">
    <div>
      <div class="repo-context">{owner}/{repo}</div>
      <h1>{mode === 'create' ? t('repo.new_file') : t('repo.edit_file')}</h1>
    </div>
    <a href={cancelHref} class="btn-secondary">{t('common.cancel')}</a>
  </div>

  {#if error}
    <div class:error-message={!conflict} class:conflict-message={conflict}>
      <span>{error}</span>
      {#if conflict}
        <button type="button" class="btn-link" onclick={() => location.reload()}>
          {t('repo.editor.reload_latest')}
        </button>
      {/if}
    </div>
  {/if}

  {#if disabledReason}
    <div class="warning-message">{disabledReason}</div>
  {/if}

  <div class="field-row">
    <label for="repo-file-path">{t('repo.editor.path')}</label>
    <input
      id="repo-file-path"
      class="path-input"
      bind:value={filePath}
      readonly={mode === 'edit'}
      placeholder={t('repo.editor.path_placeholder')}
    />
  </div>

  <div class="editor-panel surface">
    <div class="tabbar" role="tablist" aria-label={t('repo.editor.tabs_label')}>
      <button type="button" class:active={activeTab === 'edit'} onclick={() => activeTab = 'edit'}>
        {t('repo.editor.edit_tab')}
      </button>
      <button type="button" class:active={activeTab === 'preview'} onclick={() => activeTab = 'preview'}>
        {t('repo.editor.preview_tab')}
      </button>
      <button type="button" class:active={activeTab === 'diff'} onclick={() => activeTab = 'diff'}>
        {t('repo.editor.diff_tab')}
      </button>
    </div>

    {#if activeTab === 'edit'}
      <CodeEditorPane
        bind:value={fileContent}
        path={filePath}
        placeholder={t('repo.editor.content_placeholder')}
        disabled={Boolean(disabledReason)}
      />
    {:else if activeTab === 'preview'}
      <EditorPreviewPane content={fileContent} {isMarkdown} />
    {:else}
      <EditorDiffPane original={initialContent} current={fileContent} {changed} />
    {/if}
  </div>

  <div class="commit-panel">
    <div class="field-row">
      <label for="commit-message">{t('repo.editor.commit_message')}</label>
      <input
        id="commit-message"
        class="text-input"
        bind:value={commitMessage}
        placeholder={t('repo.editor.commit_placeholder')}
      />
    </div>
    <div class="field-row">
      <label for="target-branch">{t('repo.editor.branch')}</label>
      <input
        id="target-branch"
        class="text-input"
        bind:value={targetBranch}
        placeholder={t('repo.editor.branch_placeholder')}
      />
    </div>
  </div>

  <div class="form-actions">
    <button
      type="button"
      class="btn-primary"
      onclick={saveFile}
      disabled={saving || Boolean(disabledReason) || (mode === 'edit' && !changed)}
    >
      {saving
        ? t('repo.editor.saving')
        : mode === 'create'
          ? t('repo.editor.create_file')
          : t('repo.editor.save_changes')}
    </button>
    <a href={cancelHref} class="btn-secondary">{t('common.cancel')}</a>
  </div>
</div>

<style>
  .editor-shell {
    max-width: 1080px;
    margin: 0 auto;
    padding: 24px;
  }

  .editor-header {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: 16px;
    margin-bottom: 18px;
  }

  .repo-context {
    margin-bottom: 4px;
    color: var(--text-muted);
    font-size: 13px;
    font-family: ui-monospace, SFMono-Regular, "SF Mono", Menlo, Consolas, monospace;
  }

  h1 {
    margin: 0;
    line-height: 1.25;
  }

  .field-row {
    display: flex;
    flex-direction: column;
    gap: 6px;
    margin-bottom: 14px;
  }

  label {
    font-weight: 600;
    font-size: 13px;
    color: var(--text-secondary);
  }

  .path-input,
  .text-input {
    width: 100%;
    min-height: 38px;
    padding: 8px 10px;
    border: 1px solid var(--border);
    border-radius: 6px;
    background: var(--bg-primary);
    color: var(--text-primary);
    font-size: 14px;
  }

  .path-input {
    font-family: ui-monospace, SFMono-Regular, "SF Mono", Menlo, Consolas, monospace;
  }

  .path-input[readonly] {
    background: var(--bg-secondary);
    color: var(--text-secondary);
  }

  .editor-panel {
    overflow: hidden;
  }

  .tabbar {
    display: flex;
    gap: 4px;
    padding: 8px;
    border-bottom: 1px solid var(--border);
    background: var(--bg-tertiary);
  }

  .tabbar button {
    border: 1px solid transparent;
    border-radius: 5px;
    background: transparent;
    color: var(--text-secondary);
    padding: 6px 10px;
    cursor: pointer;
    font-size: 13px;
  }

  .tabbar button.active {
    background: var(--bg-primary);
    border-color: var(--border);
    color: var(--text-primary);
  }

  .path-input:focus,
  .text-input:focus {
    outline: 2px solid color-mix(in srgb, var(--accent) 35%, transparent);
    outline-offset: 1px;
  }

  .commit-panel {
    display: grid;
    grid-template-columns: minmax(0, 2fr) minmax(220px, 1fr);
    gap: 16px;
    margin-top: 18px;
  }

  .btn-link {
    min-height: 36px;
    border-radius: 6px;
    padding: 7px 12px;
    font-size: 14px;
    text-decoration: none;
    cursor: pointer;
  }

  .btn-link {
    border: none;
    background: transparent;
    color: var(--accent);
    padding-inline: 0;
  }

  .error-message,
  .conflict-message,
  .warning-message {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    margin-bottom: 14px;
    padding: 10px 12px;
    border-radius: 6px;
    font-size: 14px;
  }

  .error-message {
    border: 1px solid color-mix(in srgb, var(--red) 30%, transparent);
    background: color-mix(in srgb, var(--red) 9%, transparent);
    color: var(--red);
  }

  .conflict-message,
  .warning-message {
    border: 1px solid color-mix(in srgb, var(--yellow-dim) 35%, transparent);
    background: color-mix(in srgb, var(--yellow-dim) 11%, transparent);
    color: var(--text-primary);
  }

  @media (max-width: 720px) {
    .editor-shell {
      padding: 16px;
    }

    .editor-header,
    .commit-panel {
      display: flex;
      flex-direction: column;
    }

    .form-actions {
      flex-direction: column;
    }
  }
</style>
