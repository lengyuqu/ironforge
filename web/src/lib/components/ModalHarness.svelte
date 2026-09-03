<script lang="ts">
  // Test-only harness for Modal: snippet children + optional footer submit.
  import Modal from './Modal.svelte';

  let {
    open = true,
    title,
    submitLabel,
    submitVariant,
    submitDisabled,
    closeOnEsc = true,
    closeOnBackdrop = true,
    onSubmit,
    onClose = () => {},
  }: {
    open?: boolean;
    title?: string;
    submitLabel?: string;
    submitVariant?: 'primary' | 'danger';
    submitDisabled?: boolean;
    closeOnEsc?: boolean;
    closeOnBackdrop?: boolean;
    onSubmit?: () => void;
    onClose?: () => void;
  } = $props();
</script>

<Modal
  {open}
  {title}
  {submitLabel}
  {submitVariant}
  {submitDisabled}
  {closeOnEsc}
  {closeOnBackdrop}
  {onSubmit}
  {onClose}
>
  <p id="harness-body">Body content</p>
</Modal>
