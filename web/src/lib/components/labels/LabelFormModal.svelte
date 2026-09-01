<script lang="ts">
  // Create/edit label modal — self-contained: submits through the labels API
  // and reports success via toast. Inline formError stays inside the modal.
  import { labels } from '$lib/api/client.svelte';
  import { createT } from '$lib/i18n';
  import { toast } from '$lib/components/toast.svelte';
  import { toErrorMessage } from '$lib/utils/error';
  import type { Label } from '$lib/types/entities';

  interface Props {
    owner: string;
    repo: string;
    /** Label being edited, or null when creating. */
    label: Label | null;
    onClose: () => void;
    onSaved: () => void;
  }

  let { owner, repo, label, onClose, onSaved }: Props = $props();

  const t = createT();

  const presetColors = [
    '#ff0000', '#00ff00', '#0000ff', '#ffff00',
    '#ff00ff', '#00ffff', '#ff8800', '#888888'
  ];

  // Snapshot props before initialising $state (avoids state_referenced_locally).
  const initialName = label?.name ?? '';
  const initialColor = label?.color ?? '#ff0000';
  const initialDescription = label?.description ?? '';

  let formData = $state({
    name: initialName,
    color: initialColor,
    description: initialDescription
  });
  let saving = $state(false);
  let formError = $state('');

  function closeByKey(e: KeyboardEvent) {
    if (e.key === 'Escape' || e.key === 'Enter' || e.key === ' ') {
      e.preventDefault();
      onClose();
    }
  }

  async function handleSave() {
    if (!formData.name.trim()) {
      formError = 'Label name is required';
      return;
    }

    try {
      saving = true;
      formError = '';

      if (label) {
        await labels.update(owner, repo, label.id, {
          name: formData.name.trim(),
          color: formData.color,
          description: formData.description.trim() || undefined
        });
        toast.success(t('settings.save_label', 'Label saved'));
      } else {
        await labels.create(
          owner,
          repo,
          formData.name.trim(),
          formData.color,
          formData.description.trim() || undefined
        );
        toast.success(t('settings.create_label', 'Label created'));
      }

      onClose();
      onSaved();
    } catch (e: unknown) {
      formError = toErrorMessage(e, t('errors.update_failed', 'Update failed'));
    } finally {
      saving = false;
    }
  }
</script>

<div
  class="form-overlay"
  onclick={onClose}
  role="button"
  tabindex="0"
  onkeydown={closeByKey}
>
  <div class="form-modal" role="dialog" aria-modal="true" tabindex="-1">
    <h2>{label ? t('settings.edit_label') : t('settings.new_label')}</h2>

    {#if formError}
      <div class="error-box">{formError}</div>
    {/if}

    <div class="form-group">
      <label for="label-name">{t('settings.label_name')}</label>
      <input
        id="label-name"
        type="text"
        bind:value={formData.name}
        placeholder={t('settings.label_name_placeholder')}
        disabled={saving}
      />
    </div>

    <div class="form-group">
      <label for="label-color-input">{t('settings.label_color')}</label>

      <div class="preset-colors">
        <span class="color-section-label">{t('settings.preset_colors')}</span>
        <div class="color-swatches">
          {#each presetColors as color (color)}
            <button
              class="color-swatch"
              class:active={formData.color === color}
              style="background-color: {color}"
              onclick={() => formData.color = color}
              disabled={saving}
              aria-label="Color {color}"
            ></button>
          {/each}
        </div>
      </div>

      <div class="custom-color">
        <span class="color-section-label">{t('settings.custom_color')}</span>
        <div class="custom-color-input">
          <div class="color-preview" style="background-color: {formData.color}"></div>
          <input
            id="label-color-input"
            type="text"
            bind:value={formData.color}
            placeholder="#000000"
            disabled={saving}
            maxlength="7"
          />
        </div>
      </div>
    </div>

    <div class="form-group">
      <label for="label-desc">{t('settings.label_desc')}</label>
      <input
        id="label-desc"
        type="text"
        bind:value={formData.description}
        placeholder={t('settings.label_desc_placeholder')}
        disabled={saving}
      />
    </div>

    <div class="form-actions">
      <button class="btn btn-outline" onclick={onClose} disabled={saving}>
        Cancel
      </button>
      <button class="btn btn-primary" onclick={handleSave} disabled={saving}>
        {saving ? 'Saving...' : (label ? t('settings.save_label') : t('settings.create_label'))}
      </button>
    </div>
  </div>
</div>

<style>
  .error-box {
    padding: 0.75rem;
    background: rgba(255, 0, 0, 0.1);
    border: 1px solid var(--red, #ff4444);
    border-radius: 6px;
    color: var(--red, #ff4444);
    font-size: 0.9rem;
    margin-bottom: 1rem;
  }

  .form-overlay {
    position: fixed;
    top: 0;
    left: 0;
    right: 0;
    bottom: 0;
    background: rgba(0, 0, 0, 0.7);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 1000;
  }

  .form-modal {
    background: var(--bg-primary);
    border: 1px solid var(--border);
    border-radius: 8px;
    padding: 2rem;
    max-width: 500px;
    width: 90%;
    max-height: 90vh;
    overflow-y: auto;
  }

  .form-modal h2 {
    margin: 0 0 1.5rem 0;
    color: var(--text-primary);
    font-size: 1.25rem;
  }

  .form-group {
    margin-bottom: 1.25rem;
  }

  .form-group label {
    display: block;
    margin-bottom: 0.5rem;
    color: var(--text-primary);
    font-weight: 500;
    font-size: 0.9rem;
  }

  .form-group input[type='text'] {
    width: 100%;
    padding: 0.6rem 0.75rem;
    background: var(--bg-secondary);
    border: 1px solid var(--border);
    border-radius: 6px;
    color: var(--text-primary);
    font-size: 0.9rem;
    box-sizing: border-box;
  }

  .form-group input[type='text']:focus {
    outline: none;
    border-color: var(--accent);
  }

  .preset-colors {
    margin-bottom: 1rem;
  }

  .color-section-label {
    display: block;
    font-size: 0.85rem;
    color: var(--text-secondary);
    margin-bottom: 0.5rem;
  }

  .color-swatches {
    display: flex;
    gap: 0.5rem;
    flex-wrap: wrap;
  }

  .color-swatch {
    width: 24px;
    height: 24px;
    border-radius: 50%;
    border: 2px solid transparent;
    cursor: pointer;
    transition: all 0.2s;
    padding: 0;
  }

  .color-swatch:hover {
    transform: scale(1.1);
  }

  .color-swatch.active {
    border-color: var(--text-primary);
    box-shadow: 0 0 0 2px var(--bg-primary), 0 0 0 4px var(--text-primary);
  }

  .custom-color-input {
    display: flex;
    align-items: center;
    gap: 0.75rem;
  }

  .color-preview {
    width: 24px;
    height: 24px;
    border-radius: 50%;
    border: 1px solid var(--border);
    flex-shrink: 0;
  }

  .custom-color-input input {
    width: 100px;
    padding: 0.5rem;
    background: var(--bg-secondary);
    border: 1px solid var(--border);
    border-radius: 6px;
    color: var(--text-primary);
    font-size: 0.9rem;
    font-family: monospace;
  }

  .form-actions {
    display: flex;
    gap: 0.75rem;
    justify-content: flex-end;
    margin-top: 1.5rem;
  }
</style>
