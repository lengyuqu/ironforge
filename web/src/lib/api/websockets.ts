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

// ── Job log WebSocket with auto-reconnect (Q6.1) ────────────────────────────
//
// Mirrors the notification socket's auto-reconnect loop but adds exponential
// backoff and a log-resumption offset. The offset (`since`) is the number of
// log lines already rendered by the caller (lines = str.split('\n').length,
// empty string = 0). On reconnect the server replays the buffered log from
// the database starting at that line, so a client that survived a network
// drop resumes streaming without gaps or duplicates.
//
// Line counting stays aligned with the backend buffer, where each uploaded
// chunk is appended as `existing + "\n" + chunk`: appending chunk C to a
// buffer that had N lines yields N + countLines(C) lines, so the client just
// adds countLines(chunk) for every chunk it receives.

/** Number of lines in a log string; the empty string has zero lines. */
function countLogLines(text: string): number {
  if (!text) return 0;
  return text.split('\n').length;
}

/** Initial reconnect delay; doubles on every consecutive failure. */
const JOB_LOG_RECONNECT_BASE_MS = 1_000;
/** Upper bound for the reconnect delay. */
const JOB_LOG_RECONNECT_MAX_MS = 30_000;

type JobLogStatus = 'connected' | 'reconnecting' | 'closed';

interface JobLogSession {
  jobId: number;
  onLog: (chunk: string) => void;
  onStatus?: (status: JobLogStatus) => void;
  onError?: (err: Event) => void;
  /** Lines already delivered to the caller, used as the `since` offset. */
  receivedLines: number;
  /** Consecutive failed attempts, drives the exponential backoff. */
  attempt: number;
  reconnectEnabled: boolean;
  /** Pending reconnect timer, cleared on explicit disconnect. */
  timer: ReturnType<typeof setTimeout> | null;
  ws: WebSocket | null;
}

let jobLogSession: JobLogSession | null = null;

export function connectJobLogWebSocket(
  jobId: number,
  onLog: (chunk: string) => void,
  onStatus?: (status: JobLogStatus) => void,
  onError?: (err: Event) => void,
  /** Lines the caller already rendered (e.g. the log preloaded via REST). */
  since = 0,
): WebSocket | null {
  // Stop any previous session before starting a new one.
  disconnectJobLogWebSocket();

  const session: JobLogSession = {
    jobId,
    onLog,
    onStatus,
    onError,
    receivedLines: since,
    attempt: 0,
    reconnectEnabled: true,
    timer: null,
    ws: null,
  };
  jobLogSession = session;
  return openJobLogSocket(session);
}

function openJobLogSocket(session: JobLogSession): WebSocket | null {
  const query = session.receivedLines > 0 ? `?since=${session.receivedLines}` : '?since=0';
  const ws = new WebSocket(withWebSocketApiBase(`/ws/job/${session.jobId}${query}`));
  session.ws = ws;

  ws.onmessage = (event) => {
    try {
      const data = JSON.parse(event.data);
      if (data.type === 'connected') {
        // A successful (re)connection resets the backoff.
        session.attempt = 0;
        onJobLogStatus(session, 'connected');
        return;
      }
      if (data.event_type === 'job_log' && data.data?.job_id === session.jobId) {
        const chunk = String(data.data.log ?? '');
        session.receivedLines += countLogLines(chunk);
        session.onLog(chunk);
      }
    } catch {
      // ignore non-JSON messages
    }
  };

  ws.onerror = (err) => {
    session.onError?.(err);
  };

  ws.onclose = () => {
    if (session.ws === ws) {
      session.ws = null;
    }
    if (!session.reconnectEnabled) {
      onJobLogStatus(session, 'closed');
      return;
    }
    scheduleJobLogReconnect(session);
  };

  return ws;
}

function onJobLogStatus(session: JobLogSession, status: JobLogStatus) {
  if (session.reconnectEnabled) {
    session.onStatus?.(status);
  }
}

function scheduleJobLogReconnect(session: JobLogSession) {
  session.attempt += 1;
  const delay = Math.min(
    JOB_LOG_RECONNECT_BASE_MS * 2 ** (session.attempt - 1),
    JOB_LOG_RECONNECT_MAX_MS,
  );
  onJobLogStatus(session, 'reconnecting');
  session.timer = setTimeout(() => {
    session.timer = null;
    // Double guard: the session may have been disconnected while the timer
    // was pending, or replaced by a new connectJobLogWebSocket call.
    if (session.reconnectEnabled && jobLogSession === session) {
      openJobLogSocket(session);
    }
  }, delay);
}

export function disconnectJobLogWebSocket() {
  const session = jobLogSession;
  if (!session) return;
  session.reconnectEnabled = false;
  if (session.timer) {
    clearTimeout(session.timer);
    session.timer = null;
  }
  if (session.ws) {
    session.ws.close();
    session.ws = null;
  }
  jobLogSession = null;
}
