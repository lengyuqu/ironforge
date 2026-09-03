<script lang="ts">
  // Test-only harness: forwards props into Button with snippet children,
  // since @testing-library/svelte cannot create snippet props directly.
  import Button from './Button.svelte';

  let {
    label = 'Click',
    onclick,
    variant,
    size,
    href,
    disabled,
    class: className,
  }: {
    label?: string;
    onclick?: (e: MouseEvent) => void;
    variant?: 'primary' | 'outline' | 'ghost' | 'danger';
    size?: 'sm' | 'md' | 'lg';
    href?: string;
    disabled?: boolean;
    class?: string;
  } = $props();
</script>

<Button {variant} {size} {href} {disabled} {onclick} class={className}>{label}</Button>
