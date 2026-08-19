import { API_BASE } from './_base.svelte';

function withWebSocketApiBase(path: string): string {
  const apiUrl = new URL(API_BASE, window.location.origin);
  const protocol = apiUrl.protocol === 'https:' ? 'wss:' : 'ws:';
  const basePath = apiUrl.pathname.replace(/\/+$/g, '');
  return `${protocol}//${apiUrl.host}${basePath}${path.startsWith('/') ? path : `/${path}`}`;
}

// Module-level state controlling the auto-reconnect behaviour of the
// notification socket. `connectNotificationWebSocket` sets this to true so that
// `onclose` can schedule a reconnect; `disconnectNotificationWebSocket` flips it
// to false and closes the active connection, breaking the reconnect loop.
let notificationWs: WebSocket | null = null;
let reconnectEnabled = false;

export function connectNotificationWebSocket(
  onMessage: (event: { event_type: string; data: any }) => void,
  onError?: (err: Event) => void,
): WebSocket | null {
  // WebSocket auth uses the HttpOnly cookie sent by the browser for same-origin
  // upgrades. The backend validates the cookie before accepting the connection.
  reconnectEnabled = true;
  const ws = new WebSocket(withWebSocketApiBase('/ws/notifications'));
  notificationWs = ws;

  ws.onmessage = (event) => {
    try {
      const data = JSON.parse(event.data);
      onMessage(data);
    } catch {
      // ignore non-JSON messages
    }
  };

  ws.onerror = (err) => {
    onError?.(err);
  };

  ws.onclose = () => {
    notificationWs = null;
    // Only schedule a reconnect while it is still wanted. The double guard
    // covers the race where `disconnectNotificationWebSocket` is called while a
    // reconnect is already pending inside the setTimeout.
    if (reconnectEnabled) {
      setTimeout(() => {
        if (reconnectEnabled) {
          connectNotificationWebSocket(onMessage, onError);
        }
      }, 5000);
    }
  };

  return ws;
}

export function disconnectNotificationWebSocket() {
  reconnectEnabled = false;
  if (notificationWs) {
    notificationWs.close();
    notificationWs = null;
  }
}

export function connectJobLogWebSocket(
  jobId: number,
  onLog: (chunk: string) => void,
  onStatus?: (status: 'connected' | 'closed') => void,
  onError?: (err: Event) => void,
): WebSocket | null {
  const ws = new WebSocket(withWebSocketApiBase(`/ws/job/${jobId}`));

  ws.onmessage = (event) => {
    try {
      const data = JSON.parse(event.data);
      if (data.type === 'connected') {
        onStatus?.('connected');
        return;
      }
      if (data.event_type === 'job_log' && data.data?.job_id === jobId) {
        onLog(String(data.data.log ?? ''));
      }
    } catch {
      // ignore non-JSON messages
    }
  };

  ws.onerror = (err) => {
    onError?.(err);
  };

  ws.onclose = () => {
    onStatus?.('closed');
  };

  return ws;
}
