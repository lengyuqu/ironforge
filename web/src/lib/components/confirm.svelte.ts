// Promise-based replacement for window.confirm()/window.prompt().
//
// Usage from any component or module:
//   import { confirmDialog, promptDialog } from '$lib/components/confirm.svelte';
//   if (!(await confirmDialog({ message: question, variant: 'danger' }))) return;
//   const reason = await promptDialog({ message: question, required: true });
//
// `<ConfirmDialog />` must be rendered once in the root layout; it renders the
// request currently at the head of the queue.

export interface ConfirmOptions {
  /** Dialog title.  Defaults to a generic "Confirm action". */
  title?: string;
  message: string;
  confirmLabel?: string;
  cancelLabel?: string;
  /** `danger` styles the confirm button red and starts focus on Cancel. */
  variant?: 'primary' | 'danger';
}

export interface PromptOptions extends ConfirmOptions {
  placeholder?: string;
  defaultValue?: string;
  /** Disable the confirm button until something is typed. */
  required?: boolean;
  multiline?: boolean;
}

export interface DialogRequest {
  id: number;
  kind: 'confirm' | 'prompt';
  options: PromptOptions;
  resolve: (value: boolean | string | null) => void;
}

let nextId = 1;
const queue = $state<DialogRequest[]>([]);

function enqueue(kind: DialogRequest['kind'], options: PromptOptions) {
  return new Promise<boolean | string | null>((resolve) => {
    queue.push({ id: nextId++, kind, options, resolve });
  });
}

/** Resolves `true` on confirm, `false` on cancel / Escape / backdrop. */
export function confirmDialog(options: ConfirmOptions): Promise<boolean> {
  return enqueue('confirm', options) as Promise<boolean>;
}

/** Resolves the entered text on confirm, `null` on cancel. */
export function promptDialog(options: PromptOptions): Promise<string | null> {
  return enqueue('prompt', options) as Promise<string | null>;
}

export const dialogs = {
  get current(): DialogRequest | undefined {
    return queue[0];
  },
  settle(id: number, value: boolean | string | null) {
    const idx = queue.findIndex((r) => r.id === id);
    if (idx < 0) return;
    const [req] = queue.splice(idx, 1);
    req.resolve(value);
  },
};
