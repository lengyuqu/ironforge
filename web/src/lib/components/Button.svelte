<script lang="ts">
  import type { Snippet } from 'svelte';

  type ButtonVariant = 'primary' | 'outline' | 'ghost' | 'danger';
  type ButtonSize = 'sm' | 'md' | 'lg';

  interface Props {
    variant?: ButtonVariant;
    size?: ButtonSize;
    type?: 'button' | 'submit' | 'reset';
    disabled?: boolean;
    ariaLabel?: string;
    href?: string;
    class?: string;
    children: Snippet;
    onclick?: (e: MouseEvent) => void;
  }

  let {
    variant = 'outline',
    size = 'md',
    type = 'button',
    disabled = false,
    ariaLabel,
    href,
    class: className = '',
    children,
    onclick,
  }: Props = $props();

  // Visuals come from the global button system in app.css.
  let classes = $derived(`btn btn-${variant} btn-${size} ${className}`.trim());
</script>

{#if href}
  <a {href} class={classes} aria-label={ariaLabel}>
    {@render children()}
  </a>
{:else}
  <button {type} class={classes} {disabled} aria-label={ariaLabel} onclick={onclick}>
    {@render children()}
  </button>
{/if}
