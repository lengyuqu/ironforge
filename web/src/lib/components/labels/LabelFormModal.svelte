<script lang="ts">
  // Create/edit label modal — self-contained: submits through the labels API
  // and reports success via toast. Inline formError stays inside the modal.
  import ErrorBanner from '$lib/components/ErrorBanner.svelte';
  import Modal from '$lib/components/Modal.svelte';
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
  let nameError = $state('');
  let colorError = $state('');


  function validate(): boolean {
    nameError = formData.name.trim() ? '' : t('settings.label_name_required', 'Label name is required');
    colorError = /^#[0-9a-fA-F]{6}$/.test(formData.color.trim())
      ? ''
      : t('settings.label_color_invalid', 'Use a 6-digit hex colour such as #ff8800');
    return !nameError && !colorError;
  }

  async function handleSave() {
    if (!validate()) return;

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

<Modal
  open={true}
  title={label ? t('settings.edit_label') : t('settings.new_label')}
  submitLabel={saving ? t('common.saving') : label ? t('settings.save_label') : t('settings.create_label')}
  busy={saving}
  {onClose}
  onSubmit={handleSave}
>
  {#if formError}
    <ErrorBanner message={formError} />
  {/if}

  <div class="form-group">
    <label for="label-name">{t('settings.label_name')}</label>
    <input
      id="label-name"
      type="text"
      bind:value={formData.name}
      oninput={() => (nameError = '')}
      placeholder={t('settings.label_name_placeholder')}
      disabled={saving}
      aria-invalid={nameError ? 'true' : undefined}
      aria-describedby={nameError ? 'label-name-error' : undefined}
      data-autofocus
    />
    {#if nameError}<p class="field-error" id="label-name-error" role="alert">{nameError}</p>{/if}
  </div>

  <div class="form-group">
    <label for="label-color-input">{t('settings.label_color')}</label>

    <div class="preset-colors">
      <span class="color-section-label">{t('settings.preset_colors')}</span>
      <div class="color-swatches">
        {#each presetColors as color (color)}
          <button
            type="button"
            class="color-swatch"
            class:active={formData.color === color}
            style="background-color: {color}"
            onclick={() => { formData.color = color; colorError = ''; }}
            disabled={saving}
            aria-label="Color {color}"
            aria-pressed={formData.color === color}
          ></button>
        {/each}
      </div>
    </div>

    <div class="custom-color">
      <span class="color-section-label">{t('settings.custom_color')}</span>
      <div class="custom-color-input">
        <div class="color-preview" style="background-color: {colorError ? 'transparent' : formData.color}"></div>
        <input
          id="label-color-input"
          type="text"
          bind:value={formData.color}
          oninput={() => (colorError = '')}
          placeholder="#000000"
          disabled={saving}
          maxlength="7"
          aria-invalid={colorError ? 'true' : undefined}
          aria-describedby={colorError ? 'label-color-error' : undefined}
        />
      </div>
      {#if colorError}<p class="field-error" id="label-color-error" role="alert">{colorError}</p>{/if}
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
</Modal>

<style>




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

</style>
