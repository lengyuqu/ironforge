<script lang="ts" module>
  let fieldSeq = 0;
</script>

<script lang="ts">
  import type { Snippet } from 'svelte';
  import { createT } from '$lib/i18n';

  interface Props {
    /** Visible label text. */
    label: string;
    /** `id` of the control; generated when omitted. */
    id?: string;
    /** Helper text under the control. Hidden while an error is shown. */
    hint?: string;
    /** Validation message; also flags the control as invalid. */
    error?: string;
    /** Mark the field as required (adds an asterisk). */
    required?: boolean;
    /** Mark the field as optional (adds "(optional)"). */
    optional?: boolean;
    /** The control. Receives the wiring to spread onto it. */
    children: Snippet<[{ id: string; describedBy: string | undefined; invalid: boolean }]>;
  }

  let { label, id: idProp, hint, error, required = false, optional = false, children }: Props = $props();

  const t = createT();
  const generatedId = `field-${++fieldSeq}`;

  let id = $derived(idProp ?? generatedId);
  let describedBy = $derived(error ? `${id}-error` : hint ? `${id}-hint` : undefined);
</script>

<div class="form-group">
  <label for={id}>
    {label}
    {#if required}<span class="field-required" aria-hidden="true">*</span>{/if}
    {#if optional}<span class="field-optional">{t('common.optional')}</span>{/if}
  </label>
  {@render children({ id, describedBy, invalid: !!error })}
  {#if error}
    <p class="field-error" id="{id}-error" role="alert">{error}</p>
  {:else if hint}
    <p class="form-hint" id="{id}-hint">{hint}</p>
  {/if}
</div>

<style>
  .field-required {
    margin-left: 2px;
    color: var(--red);
  }

  .field-optional {
    margin-left: 4px;
    font-weight: 400;
    color: var(--text-muted);
  }
</style>
