<script lang="ts">
  // Release form — self-contained: submit through the releases API and
  // report failures via toast. Dual mode: when the `release` prop is
  // provided it edits an existing release (tag locked, no target picker);
  // otherwise it creates a new one (tag + target_commitish selectable).
  import FormField from '$lib/components/FormField.svelte';
  import TagHints from '$lib/components/releases/TagHints.svelte';
  import ReleaseTargetPicker from '$lib/components/releases/ReleaseTargetPicker.svelte';
  import { releases } from '$lib/api/client.svelte';
  import { toast } from '$lib/components/toast.svelte';
  import { toErrorMessage } from '$lib/utils/error';
  import { createT } from '$lib/i18n';
  import type { Release } from '$lib/types/entities';

  const t = createT();

  let {
    owner,
    repo,
    branches = [],
    tags = [],
    /** Release being edited, or null/undefined when creating. */
    release = null,
    /** Called after a successful create. */
    onCreated,
    /** Called after a successful update. */
    onSaved,
  }: {
    owner: string;
    repo: string;
    branches?: string[];
    tags?: string[];
    release?: Release | null;
    onCreated?: () => void | Promise<void>;
    onSaved?: () => void | Promise<void>;
  } = $props();

  const isEdit = release != null;

  // Snapshot props before initialising $state (avoids state_referenced_locally).
  const initialTagName = release?.tag_name ?? '';
  const initialTitle = release?.title ?? '';
  const initialBody = release?.body ?? '';
  const initialDraft = !!release?.is_draft;
  const initialPrerelease = !!release?.is_prerelease;

  let tagName = $state(initialTagName);
  let releaseTitle = $state(initialTitle);
  let body = $state(initialBody);
  let isDraft = $state(initialDraft);
  let isPrerelease = $state(initialPrerelease);
  let targetCommitish = $state('');
  let submitting = $state(false);

  async function handleSubmit(e: Event) {
    e.preventDefault();

    if (!isEdit && !tagName.trim()) {
      toast.error(t('releases.tag_name_required', 'Tag name is required'));
      return;
    }

    if (!releaseTitle.trim()) {
      toast.error(t('releases.release_title_required', 'Release title is required'));
      return;
    }

    submitting = true;

    try {
      if (isEdit && release) {
        await releases.update(owner, repo, release.id, {
          title: releaseTitle.trim(),
          body: body.trim() || undefined,
          is_draft: isDraft,
          is_prerelease: isPrerelease,
        });
        toast.success(t('releases.updated', 'Release updated'));
        await onSaved?.();
      } else {
        await releases.create(owner, repo, {
          tag_name: tagName.trim(),
          title: releaseTitle.trim(),
          body: body.trim() || undefined,
          target_commitish: targetCommitish || undefined,
          is_draft: isDraft,
          is_prerelease: isPrerelease,
        });
        await onCreated?.();
      }
    } catch (e) {
      toast.error(toErrorMessage(e, t('errors.save_failed', 'Save failed')));
      submitting = false;
    }
  }
</script>

<form class="release-form surface" onsubmit={handleSubmit}>
  <div class="form-group">
    <label for="tag-name">{t('releases.tag_name')} <span class="required">*</span></label>
    <input
      type="text"
      id="tag-name"
      bind:value={tagName}
      placeholder={t('releases.tag_name_placeholder')}
      required
      class="input"
      disabled={isEdit}
    />
    {#if !isEdit}
      <TagHints {tags} onPick={(tag) => (tagName = tag)} />
    {/if}
  </div>

  <div class="form-group">
    <label for="release-title">{t('releases.release_title')} <span class="required">*</span></label>
    <input
      type="text"
      id="release-title"
      bind:value={releaseTitle}
      placeholder={t('releases.release_title_placeholder')}
      required
      class="input"
    />
  </div>

  <FormField id="body" label={t('releases.body')}>
    {#snippet children()}
      <textarea
        id="body"
        bind:value={body}
        placeholder={t('releases.body_placeholder')}
        rows="8"
        class="textarea"
      ></textarea>
    {/snippet}
  </FormField>

  {#if !isEdit}
    <ReleaseTargetPicker {tags} {branches} bind:value={targetCommitish} />
  {/if}

  <div class="form-group checkbox-group">
    <label class="checkbox-label">
      <input type="checkbox" bind:checked={isDraft} />
      <span>{t('releases.is_draft')}</span>
    </label>
  </div>

  <div class="form-group checkbox-group">
    <label class="checkbox-label">
      <input type="checkbox" bind:checked={isPrerelease} />
      <span>{t('releases.is_prerelease')}</span>
    </label>
  </div>

  <div class="form-actions">
    <a href={`/${owner}/${repo}/releases`} class="btn-secondary">{t('common.cancel')}</a>
    <button type="submit" class="btn-primary" disabled={submitting}>
      {submitting
        ? t('releases.submitting')
        : isEdit
          ? t('releases.save', 'Save')
          : t('releases.submit')}
    </button>
  </div>
</form>

<style>
  .release-form {
    padding: 24px;
  }

  .required {
    color: var(--red);
  }

  .input,
  .textarea {
    width: 100%;
    box-sizing: border-box;
  }

  .input:disabled {
    cursor: not-allowed;
  }

  .textarea {
    resize: vertical;
    min-height: 120px;
  }

  .checkbox-group {
    margin-bottom: 12px;
  }

  .checkbox-label {
    font-weight: 500;
  }

  .checkbox-label input[type="checkbox"] {
    width: 16px;
    height: 16px;
    cursor: pointer;
  }

  .form-actions {
    justify-content: flex-end;
    padding-top: 20px;
    border-top: 1px solid var(--border);
  }

  @media (max-width: 600px) {
    .release-form {
      padding: 16px;
    }

    .form-actions {
      flex-direction: column-reverse;
    }
  }
</style>
