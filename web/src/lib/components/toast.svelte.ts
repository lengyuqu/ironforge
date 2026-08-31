// Toast notification manager — Svelte 5 runes.
//
// Usage from any component:
//   import { toast } from '$lib/components/toast';
//   toast.show({ message: 'Saved!', type: 'success' });
//   toast.success('Saved!');
//   toast.error('Failed: ' + err.message);
//
// Render `<ToastContainer />` once in the root layout — all toasts stack there.

export type ToastType = 'info' | 'success' | 'warning' | 'error';

export interface ToastItem {
  id: number;
  message: string;
  type: ToastType;
  /** Auto-dismiss after this many ms. 0 = no auto-dismiss. */
  duration: number;
}

const DEFAULT_DURATIONS: Record<ToastType, number> = {
  info: 4000,
  success: 3000,
  warning: 5000,
  error: 8000,
};

let nextId = 1;
const items = $state<ToastItem[]>([]);

function push(message: string, type: ToastType = 'info', duration?: number): ToastItem {
  const id = nextId++;
  const item: ToastItem = {
    id,
    message,
    type,
    duration: duration ?? DEFAULT_DURATIONS[type],
  };
  items.push(item);
  if (item.duration > 0) {
    setTimeout(() => dismiss(id), item.duration);
  }
  return item;
}

function dismiss(id: number) {
  const idx = items.findIndex((t) => t.id === id);
  if (idx >= 0) items.splice(idx, 1);
}

export const toast = {
  get items() {
    return items;
  },
  show(opts: { message: string; type?: ToastType; duration?: number }) {
    return push(opts.message, opts.type ?? 'info', opts.duration);
  },
  info(message: string, duration?: number) {
    return push(message, 'info', duration);
  },
  success(message: string, duration?: number) {
    return push(message, 'success', duration);
  },
  warning(message: string, duration?: number) {
    return push(message, 'warning', duration);
  },
  error(message: string, duration?: number) {
    return push(message, 'error', duration);
  },
  dismiss,
  clear() {
    items.length = 0;
  },
};
