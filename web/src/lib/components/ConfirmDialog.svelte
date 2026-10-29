<script lang="ts">
  import Modal from './Modal.svelte';
  import { dialogs } from './confirm.svelte';
  import { createT } from '$lib/i18n';

  const t = createT();

  let current = $derived(dialogs.current);
  let value = $state('');
  let lastId = -1;

  // Reset the input whenever a new request reaches the head of the queue.
  $effect(() => {
    if (current && current.id !== lastId) {
      lastId = current.id;
      value = current.options.defaultValue ?? '';
    }
  });

  function cancel() {
    if (current) dialogs.settle(current.id, current.kind === 'prompt' ? null : false);
  }

  function accept() {
    if (!current) return;
    dialogs.settle(current.id, current.kind === 'prompt' ? value.trim() : true);
  }

  let blocked = $derived(
    !!current && current.kind === 'prompt' && !!current.options.required && value.trim() === '',
  );
</script>

{#if current}
  {#key current.id}
    <Modal
      open={true}
      class="modal-narrow"
      title={current.options.title ?? t('common.confirm_action')}
      submitLabel={current.options.confirmLabel ??
        (current.options.variant === 'danger' ? t('common.delete') : t('common.confirm'))}
      submitVariant={current.options.variant ?? 'primary'}
      submitDisabled={blocked}
      onClose={cancel}
      onSubmit={accept}
    >
      <p class="confirm-message">{current.options.message}</p>
      {#if current.kind === 'prompt'}
        {#if current.options.multiline}
          <textarea
            class="confirm-input"
            rows="3"
            placeholder={current.options.placeholder}
            bind:value
            data-autofocus
          ></textarea>
        {:else}
          <input
            class="confirm-input"
            type="text"
            placeholder={current.options.placeholder}
            bind:value
            data-autofocus
          />
        {/if}
      {/if}
    </Modal>
  {/key}
{/if}

<style>
  .confirm-message {
    margin: 0;
    line-height: 1.6;
    color: var(--text-primary);
    white-space: pre-line;
    word-break: break-word;
  }

  .confirm-input {
    display: block;
    width: 100%;
    margin-top: 14px;
  }
</style>
