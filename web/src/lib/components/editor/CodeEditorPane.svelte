<script lang="ts">
  import { escapeHtml, languageFromPath } from '$lib/utils/editorFile';

  interface Props {
    value: string;
    /** File path; only its extension is used to pick the highlight language. */
    path: string;
    disabled?: boolean;
    placeholder?: string;
  }

  let { value = $bindable(), path, disabled = false, placeholder = '' }: Props = $props();

  let textarea = $state<HTMLTextAreaElement | null>(null);
  let backdrop = $state<HTMLPreElement | null>(null);
  let highlighted = $state('');
  let requestId = 0;

  let language = $derived(languageFromPath(path));

  // The textarea is transparent and sits over a highlighted <pre>; re-highlight
  // whenever the text or the language changes.
  $effect(() => {
    value;
    language;
    void updateHighlight();
  });

  async function updateHighlight() {
    const current = ++requestId;
    const content = value || ' ';
    const lang = language;

    try {
      // Loaded lazily so highlight.js stays out of the main bundle.
      const hljs = await import('highlight.js');
      if (current !== requestId) return;

      const highlighter = hljs.default;
      highlighted =
        lang && highlighter.getLanguage(lang)
          ? highlighter.highlight(content, { language: lang }).value
          : highlighter.highlightAuto(content).value;
    } catch {
      if (current === requestId) highlighted = escapeHtml(content);
    } finally {
      syncScroll();
    }
  }

  function syncScroll() {
    if (!textarea || !backdrop) return;
    backdrop.scrollTop = textarea.scrollTop;
    backdrop.scrollLeft = textarea.scrollLeft;
  }
</script>

<div class="code-editor-wrap">
  <pre class="highlight-backdrop" aria-hidden="true" bind:this={backdrop}><code
      class="hljs language-{language}">{@html highlighted}</code></pre>
  <textarea
    id="file-content"
    bind:this={textarea}
    bind:value
    class="file-editor"
    rows="22"
    spellcheck="false"
    autocapitalize="off"
    autocomplete="off"
    {placeholder}
    {disabled}
    onscroll={syncScroll}
  ></textarea>
</div>

<style>
  .code-editor-wrap {
    position: relative;
    min-height: 520px;
    background: var(--bg-primary);
  }

  .highlight-backdrop,
  .file-editor {
    width: 100%;
    min-height: 520px;
    padding: 14px;
    margin: 0;
    border: none;
    font-family: ui-monospace, SFMono-Regular, "SF Mono", Menlo, Consolas, monospace;
    font-size: 13px;
    line-height: 1.55;
    tab-size: 4;
    white-space: pre;
  }

  .highlight-backdrop {
    position: absolute;
    inset: 0;
    overflow: hidden;
    color: var(--text-primary);
    pointer-events: none;
    user-select: none;
  }

  .highlight-backdrop code {
    font-family: inherit;
    font-size: inherit;
    line-height: inherit;
    white-space: pre;
  }

  .file-editor {
    position: relative;
    z-index: 1;
    resize: vertical;
    background: transparent;
    color: transparent;
    caret-color: var(--text-primary);
    overflow: auto;
  }

  .file-editor::selection {
    background: color-mix(in srgb, var(--accent) 28%, transparent);
    color: transparent;
  }

  .file-editor::placeholder {
    color: var(--text-muted);
  }

  .file-editor:disabled {
    opacity: 0.72;
  }

  .file-editor:focus {
    outline: 2px solid color-mix(in srgb, var(--accent) 35%, transparent);
    outline-offset: 1px;
  }
</style>
