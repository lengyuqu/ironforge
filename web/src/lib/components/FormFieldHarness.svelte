<script lang="ts">
  // Test-only harness: FormField takes a snippet child, which the testing
  // library cannot pass directly.
  import FormField from './FormField.svelte';

  let {
    label = 'Name',
    hint,
    error,
    required = false,
    optional = false,
    id,
  }: { label?: string; hint?: string; error?: string; required?: boolean; optional?: boolean; id?: string } = $props();
</script>

<FormField {label} {hint} {error} {required} {optional} {id}>
  {#snippet children({ id: fieldId, describedBy, invalid })}
    <input id={fieldId} aria-describedby={describedBy} aria-invalid={invalid ? 'true' : undefined} />
  {/snippet}
</FormField>
