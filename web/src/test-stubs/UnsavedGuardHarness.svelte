<script lang="ts">
  import { onMount } from 'svelte';
  import { useUnsavedChangesGuard, type UnsavedChangesGuard } from '$lib/utils/unsavedChanges.svelte';

  interface Props {
    dirty: boolean;
    onGuard?: (guard: UnsavedChangesGuard) => void;
  }

  let { dirty, onGuard }: Props = $props();

  const guard = useUnsavedChangesGuard(() => dirty);

  onMount(() => {
    onGuard?.(guard);
  });
</script>
