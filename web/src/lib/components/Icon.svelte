<script lang="ts" module>
  export type IconName =
    | 'code'
    | 'issue'
    | 'pull-request'
    | 'book'
    | 'play'
    | 'tag'
    | 'package'
    | 'kanban'
    | 'clock'
    | 'agents'
    | 'git-commit'
    | 'settings'
    | 'star'
    | 'star-filled'
    | 'eye'
    | 'eye-off'
    | 'git-fork'
    | 'chevron-down'
    | 'chevron-right'
    | 'folder'
    | 'folder-up'
    | 'file'
    | 'search'
    | 'globe'
    | 'shield'
    | 'key'
    | 'lock'
    | 'rocket'
    | 'trash'
    | 'repeat'
    | 'bell'
    | 'users'
    | 'building'
    | 'clipboard'
    | 'activity'
    | 'cpu'
    | 'zap'
    | 'layers'
    | 'server'
    | 'puzzle'
    | 'check'
    | 'x'
    | 'target';
</script>

<script lang="ts">
  interface Props {
    name: IconName;
    size?: number;
    class?: string;
  }

  let { name, size = 16, class: className = '' }: Props = $props();
</script>

<svg
  class="icon {className}"
  width={size}
  height={size}
  viewBox="0 0 16 16"
  fill="none"
  stroke="currentColor"
  stroke-width="1.5"
  stroke-linecap="round"
  stroke-linejoin="round"
  aria-hidden="true"
>
  {#if name === 'code'}
    <path d="M5.5 4.5 2 8l3.5 3.5M10.5 4.5 14 8l-3.5 3.5M9 3l-2 10" />
  {:else if name === 'issue'}
    <circle cx="8" cy="8" r="6" />
    <circle cx="8" cy="8" r="1.2" fill="currentColor" stroke="none" />
  {:else if name === 'pull-request'}
    <circle cx="4.5" cy="4" r="1.75" />
    <circle cx="4.5" cy="12" r="1.75" />
    <circle cx="11.5" cy="12" r="1.75" />
    <path d="M4.5 5.75v4.5M8.5 4h1.5A1.5 1.5 0 0 1 11.5 5.5v4.75" />
    <path d="m9.75 2.5-1.5 1.5 1.5 1.5" />
  {:else if name === 'book'}
    <path d="M2.5 3.5A1.5 1.5 0 0 1 4 2h3.5v11.5H4a1.5 1.5 0 0 0-1.5 1.5V3.5ZM13.5 3.5A1.5 1.5 0 0 0 12 2H8.5v11.5H12a1.5 1.5 0 0 1 1.5 1.5V3.5Z" />
  {:else if name === 'play'}
    <circle cx="8" cy="8" r="6" />
    <path d="m6.75 5.5 4 2.5-4 2.5V5.5Z" fill="currentColor" stroke="none" />
  {:else if name === 'tag'}
    <path d="M2.5 8.5V3.5a1 1 0 0 1 1-1h5l5 5a1.2 1.2 0 0 1 0 1.7l-3.3 3.3a1.2 1.2 0 0 1-1.7 0l-6-4Z" />
    <circle cx="5.75" cy="5.75" r="0.9" fill="currentColor" stroke="none" />
  {:else if name === 'package'}
    <path d="M8 1.8 13.5 4.9v6.2L8 14.2 2.5 11.1V4.9L8 1.8Z" />
    <path d="M2.5 4.9 8 8l5.5-3.1M8 8v6.2" />
  {:else if name === 'kanban'}
    <rect x="2" y="2.5" width="12" height="11" rx="1.5" />
    <path d="M5.5 5.5v5M8.5 5.5v3M11.5 5.5v4" />
  {:else if name === 'clock'}
    <circle cx="8" cy="8" r="6" />
    <path d="M8 4.8V8l2.2 1.4" />
  {:else if name === 'agents'}
    <rect x="3.2" y="4.5" width="9.6" height="7.5" rx="2" />
    <path d="M8 2v2.5M6 8h.01M10 8h.01M6.2 10.2h3.6" />
  {:else if name === 'git-commit'}
    <circle cx="8" cy="8" r="2.25" />
    <path d="M1.5 8h4.25M10.25 8h4.25" />
  {:else if name === 'settings'}
    <circle cx="8" cy="8" r="2.1" />
    <path d="M8 1.8v1.4M8 12.8v1.4M2.7 4.9l1.2.7M12.1 10.4l1.2.7M2.7 11.1l1.2-.7M12.1 5.6l1.2-.7" />
  {:else if name === 'star'}
    <path d="m8 2 1.85 3.75 4.15.6-3 2.92.71 4.13L8 11.45 4.29 13.4 5 9.27 2 6.35l4.15-.6L8 2Z" />
  {:else if name === 'star-filled'}
    <path
      fill="currentColor"
      d="m8 2 1.85 3.75 4.15.6-3 2.92.71 4.13L8 11.45 4.29 13.4 5 9.27 2 6.35l4.15-.6L8 2Z"
    />
  {:else if name === 'eye'}
    <path d="M1.5 8S4 3.5 8 3.5 14.5 8 14.5 8 12 12.5 8 12.5 1.5 8 1.5 8Z" />
    <circle cx="8" cy="8" r="1.8" />
  {:else if name === 'eye-off'}
    <path d="M2 2 14 14M6.5 3.8A6.8 6.8 0 0 1 8 3.5c4 0 6.5 4.5 6.5 4.5a11.3 11.3 0 0 1-2.1 2.7M4.2 5.4A11.2 11.2 0 0 0 1.5 8s2.5 4.5 6.5 4.5c1 0 1.9-.2 2.7-.6" />
  {:else if name === 'git-fork'}
    <circle cx="4.5" cy="3.8" r="1.6" />
    <circle cx="11.5" cy="3.8" r="1.6" />
    <circle cx="8" cy="12.2" r="1.6" />
    <path d="M4.5 5.4v1.1A1.5 1.5 0 0 0 6 8h4a1.5 1.5 0 0 0 1.5-1.5V5.4M8 8v2.6" />
  {:else if name === 'chevron-down'}
    <path d="m4 6 4 4 4-4" />
  {:else if name === 'chevron-right'}
    <path d="m6 4 4 4-4 4" />
  {:else if name === 'folder'}
    <path d="M2 4.2A1.2 1.2 0 0 1 3.2 3h3l1.4 1.5h5.2A1.2 1.2 0 0 1 14 5.7v6.6a1.2 1.2 0 0 1-1.2 1.2H3.2A1.2 1.2 0 0 1 2 12.3V4.2Z" />
  {:else if name === 'folder-up'}
    <path d="M2 4.2A1.2 1.2 0 0 1 3.2 3h3l1.4 1.5h5.2A1.2 1.2 0 0 1 14 5.7v6.6a1.2 1.2 0 0 1-1.2 1.2H3.2A1.2 1.2 0 0 1 2 12.3V4.2Z" />
    <path d="M8 10.8V7.2M6.2 8.8 8 7l1.8 1.8" />
  {:else if name === 'file'}
    <path d="M4 2.2h5.2L12.5 5.5v8.3a1 1 0 0 1-1 1H4a1 1 0 0 1-1-1V3.2a1 1 0 0 1 1-1Z" />
    <path d="M9 2.2v3.5h3.5" />
  {:else if name === 'search'}
    <circle cx="7" cy="7" r="4.5" />
    <path d="m10.4 10.4 3.4 3.4" />
  {:else if name === 'globe'}
    <circle cx="8" cy="8" r="6" />
    <path d="M2.2 8h11.6M8 2c1.8 2 1.8 10 0 12M8 2c-1.8 2-1.8 10 0 12" />
  {:else if name === 'shield'}
    <path d="M8 1.8 13.2 4v3.8c0 3.3-2.2 5.4-5.2 6.4-3-1-5.2-3.1-5.2-6.4V4L8 1.8Z" />
  {:else if name === 'key'}
    <circle cx="5.5" cy="9.5" r="2.7" />
    <path d="m7.5 7.5 5.5-5.5M11 4l1.8 1.8M9.5 5.5l1.3 1.3" />
  {:else if name === 'lock'}
    <rect x="3" y="7" width="10" height="6.8" rx="1.4" />
    <path d="M5.2 7V5a2.8 2.8 0 0 1 5.6 0v2" />
  {:else if name === 'rocket'}
    <path d="M9.5 2.2c2.3 0 4.3 2 4.3 4.3-1.4 3.1-3.8 5.5-6.9 6.9L4.6 11.1c1.4-3.1 3.8-5.5 6.9-6.9Z" />
    <circle cx="9.6" cy="6.4" r="1" />
    <path d="M5.2 9.8 2.8 10.4l1.5-2.7M6.2 10.8l-.6 2.4 2.7-1.5" />
  {:else if name === 'trash'}
    <path d="M2.5 4.2h11M5.8 4.2V2.6h4.4v1.6M4.2 4.2l.7 9a1 1 0 0 0 1 .9h4.2a1 1 0 0 0 1-.9l.7-9" />
  {:else if name === 'repeat'}
    <path d="m11.8 2.5 2 2-2 2M13.8 4.5H5A2.5 2.5 0 0 0 2.5 7v1" />
    <path d="m4.2 13.5-2-2 2-2M2.2 11.5H11a2.5 2.5 0 0 0 2.5-2.5V8" />
  {:else if name === 'bell'}
    <path d="M4.2 6.8a3.8 3.8 0 1 1 7.6 0c0 3 1.2 4.2 1.2 4.2H3s1.2-1.2 1.2-4.2Z" />
    <path d="M6.5 13a1.6 1.6 0 0 0 3 0" />
  {:else if name === 'users'}
    <circle cx="6" cy="5.5" r="2.2" />
    <path d="M2.2 13.2a3.8 3.8 0 0 1 7.6 0" />
    <circle cx="11.2" cy="6.2" r="1.7" />
    <path d="M11 10.1a3 3 0 0 1 3 3.1" />
  {:else if name === 'building'}
    <rect x="3" y="2.2" width="10" height="11.6" rx="1" />
    <path d="M6 5h1M9 5h1M6 8h1M9 8h1M6.8 13.8v-2.8h2.4v2.8" />
  {:else if name === 'clipboard'}
    <rect x="3.2" y="3.2" width="9.6" height="11" rx="1.2" />
    <path d="M6 2h4v2H6zM5.8 7.5h4.4M5.8 10.5h3.2" />
  {:else if name === 'activity'}
    <path d="M1.8 8.2h2.6l1.7-4.4 2.8 8.4 1.9-4h3.4" />
  {:else if name === 'cpu'}
    <rect x="4.2" y="4.2" width="7.6" height="7.6" rx="1" />
    <path d="M6.2 1.8v2.4M9.8 1.8v2.4M6.2 11.8v2.4M9.8 11.8v2.4M1.8 6.2h2.4M1.8 9.8h2.4M11.8 6.2h2.4M11.8 9.8h2.4" />
  {:else if name === 'zap'}
    <path d="M8.8 1.8 3.5 8.8h4.3L7.2 14.2l5.3-7H8.2l.6-5.4Z" />
  {:else if name === 'layers'}
    <path d="M8 2 14 5.2 8 8.4 2 5.2 8 2Z" />
    <path d="m2 8.2 6 3.2 6-3.2M2 11.2l6 3.2 6-3.2" />
  {:else if name === 'server'}
    <rect x="2.2" y="2.5" width="11.6" height="4.5" rx="1" />
    <rect x="2.2" y="9" width="11.6" height="4.5" rx="1" />
    <circle cx="4.8" cy="4.75" r="0.6" fill="currentColor" stroke="none" />
    <circle cx="4.8" cy="11.25" r="0.6" fill="currentColor" stroke="none" />
  {:else if name === 'puzzle'}
    <path d="M5.5 3h3a1.5 1.5 0 0 1 0 3H12v3.5a1.5 1.5 0 0 1 0 3V14H3.5V9.5a1.5 1.5 0 0 0 0-3V3h2Z" />
  {:else if name === 'target'}
    <circle cx="8" cy="8" r="6" />
    <circle cx="8" cy="8" r="3" />
    <circle cx="8" cy="8" r="0.9" fill="currentColor" stroke="none" />
  {:else if name === 'check'}
    <path d="m3 8.2 3.2 3.2L13 4.8" />
  {:else if name === 'x'}
    <path d="m4 4 8 8M12 4l-8 8" />
  {/if}
</svg>

<style>
  .icon {
    flex-shrink: 0;
    vertical-align: middle;
  }
</style>
