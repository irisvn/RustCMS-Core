import type { ApiClient } from "@/api/client";
import { extensionUrl } from "./paths";

/** What a stream reports besides its events. */
export type EventsState =
  | { state: "open" }
  | { state: "reconnecting"; status?: number; reason?: string }
  | { state: "closed"; status?: number; reason?: string };

export interface EventsHandlers {
  /** Each event (`resync`: reload what the page shows). */
  onEvent: (name: string, data: unknown) => void;
  onState?: (state: EventsState) => void;
}

export interface EventsHandle {
  /** Stops the stream for good (also on unmount and on any session change). */
  close: () => void;
}

/** Reasons a stream never reconnects after. */
const FINAL_REASONS = new Set(["signed_out", "forbidden"]);
/** Statuses a stream never reconnects after. */
const FINAL_STATUSES = new Set([400, 401, 403, 404, 405]);

export interface EventsOptions {
  /** First and longest wait before reconnecting (ms). */
  minDelay?: number;
  maxDelay?: number;
}

/**
 * Server-sent events of an admin route (`AdminRoutes::events`), read with
 * `fetch` (statuses are visible and nothing reconnects on its own, unlike
 * `EventSource`). Reconnects after a network drop or a server end with
 * jittered backoff; stops for good on 401/403/404 or `closed: signed_out |
 * forbidden`, on `close()`, and as soon as the session epoch changes.
 */
export function openEvents(
  api: ApiClient,
  path: string,
  handlers: EventsHandlers,
  options: EventsOptions = {},
): EventsHandle {
  // The shared extension-path rules (one leading slash, each segment checked).
  const url = extensionUrl(api.apiPath, path);
  const minDelay = options.minDelay ?? 1000;
  const maxDelay = options.maxDelay ?? 30_000;
  const epoch = api.epoch;
  let stopped = false;
  let controller: AbortController | null = null;
  let timer: ReturnType<typeof setTimeout> | null = null;
  let failures = 0;
  const live = () => !stopped && api.epoch === epoch;
  const report = (state: EventsState) => {
    if (live() || state.state === "closed") handlers.onState?.(state);
  };
  const stop = (final?: EventsState) => {
    if (stopped) return;
    stopped = true;
    unsubscribe();
    controller?.abort();
    if (timer) clearTimeout(timer);
    if (final) handlers.onState?.(final);
  };
  const unsubscribe = api.onEpochChange(() => stop({ state: "closed", reason: "session" }));
  const retry = (detail: { status?: number; reason?: string }) => {
    if (!live()) return;
    failures += 1;
    const delay = Math.min(maxDelay, minDelay * 2 ** (failures - 1)) * (0.5 + Math.random() / 2);
    report({ state: "reconnecting", ...detail });
    timer = setTimeout(() => {
      timer = null;
      void connect();
    }, delay);
  };
  const connect = async () => {
    if (!live()) return;
    controller = new AbortController();
    let response: Response;
    try {
      response = await api.fetcher(url, {
        credentials: "same-origin",
        headers: { Accept: "text/event-stream" },
        signal: controller.signal,
      });
    } catch {
      return retry({});
    }
    if (!live()) return;
    if (FINAL_STATUSES.has(response.status)) return stop({ state: "closed", status: response.status });
    // Another account's stream (the cookie changed under this tab): never shown.
    if (!api.acceptsResponse(response)) {
      controller.abort();
      return stop({ state: "closed", reason: "session" });
    }
    if (!response.ok || !response.body) return retry({ status: response.status });
    report({ state: "open" });
    const reader = response.body.getReader();
    const decoder = new TextDecoder();
    let buffer = "";
    let reason: string | undefined;
    try {
      reading: for (;;) {
        const { value, done } = await reader.read();
        if (done || !live()) break;
        buffer += decoder.decode(value, { stream: true });
        // CR LF may be split across chunks: a trailing CR waits for the next one.
        const pending = buffer.endsWith("\r") ? "\r" : "";
        buffer = buffer.slice(0, buffer.length - pending.length).replace(/\r\n?/g, "\n") + pending;
        let end: number;
        while ((end = buffer.indexOf("\n\n")) >= 0) {
          const block = buffer.slice(0, end);
          buffer = buffer.slice(end + 2);
          const event = parse(block);
          if (!event || !live()) continue;
          if (event.name === "closed") {
            // The server ended it: nothing after this counts.
            reason = (event.data as { reason?: string } | null)?.reason ?? "closed";
            controller.abort();
            break reading;
          }
          failures = 0;
          handlers.onEvent(event.name, event.data);
        }
      }
    } catch {
      // A dropped connection: retried below.
    }
    if (!live()) return;
    if (reason && FINAL_REASONS.has(reason)) return stop({ state: "closed", reason });
    retry({ reason });
  };
  void connect();
  return { close: () => stop() };
}

/** One SSE block: `event:` and `data:` lines (comments and other fields ignored). */
function parse(block: string): { name: string; data: unknown } | null {
  let name = "message";
  const data: string[] = [];
  for (const line of block.split("\n")) {
    if (line.startsWith(":")) continue;
    const colon = line.indexOf(":");
    const field = colon < 0 ? line : line.slice(0, colon);
    const value = colon < 0 ? "" : line.slice(colon + 1).replace(/^ /, "");
    if (field === "event") name = value;
    else if (field === "data") data.push(value);
  }
  if (data.length === 0) return null;
  const text = data.join("\n");
  try {
    return { name, data: JSON.parse(text) };
  } catch {
    return { name, data: text };
  }
}

/** What a socket reports besides its messages. */
export type SocketState =
  | { state: "open" }
  | { state: "reconnecting"; code?: number; reason?: string }
  | { state: "closed"; code?: number; reason?: string };

export interface SocketHandlers {
  /** Each message: a string (text) or an `ArrayBuffer` (binary). */
  onMessage: (data: string | ArrayBuffer) => void;
  onOpen?: () => void;
  onState?: (state: SocketState) => void;
}

export interface SocketHandle {
  /**
   * Sends `data` while open; false when not open, larger than the limit, or
   * when it would queue more than 1 MiB (nothing is kept for later).
   */
  send: (data: string | ArrayBuffer | ArrayBufferView) => boolean;
  /** Closes the socket for good (also on unmount and on any session change). */
  close: () => void;
}

export interface SocketOptions {
  /** Reconnect after a drop (off: a new socket is a new session on the server). */
  reconnect?: boolean;
  /** Largest message sent (`[realtime] max_message_bytes`). */
  maxMessageBytes?: number;
  minDelay?: number;
  maxDelay?: number;
  /** The `WebSocket` constructor (tests). */
  WebSocket?: typeof WebSocket;
}

/** Close codes a socket never reconnects after (signed out, forbidden). */
const FINAL_CODES = new Set([4401, 4403]);
/** Most bytes queued in the browser by `send`. */
const MAX_BUFFERED = 1024 * 1024;
/** Failed or short connections before reconnecting stops. */
const MAX_FAILURES = 5;
/** A connection open this long counts as healthy (resets the failures). */
const HEALTHY_MS = 30_000;

function byteLength(data: string | ArrayBuffer | ArrayBufferView): number {
  return typeof data === "string" ? new TextEncoder().encode(data).length : data.byteLength;
}

/**
 * A WebSocket of an admin route (`AdminRoutes::socket`), on the same origin,
 * carrying the session's CSRF token as a subprotocol. Closed for good on
 * `close()`, on 4401/4403, and as soon as the session epoch changes; input is
 * never replayed. With `reconnect`, a drop is retried with jittered backoff
 * and a refreshed token; failed handshakes and connections shorter than 30 s
 * count toward 5 failures, after which it stops.
 */
export function openSocket(
  api: ApiClient,
  path: string,
  handlers: SocketHandlers,
  options: SocketOptions = {},
): SocketHandle {
  const pathname = new URL(extensionUrl(api.apiPath, path), globalThis.location?.origin ?? "http://localhost").pathname;
  const Socket = options.WebSocket ?? globalThis.WebSocket;
  const scheme = globalThis.location?.protocol === "https:" ? "wss" : "ws";
  const url = `${scheme}://${globalThis.location?.host ?? "localhost"}${pathname}`;
  const maxBytes = options.maxMessageBytes ?? 64 * 1024;
  const minDelay = options.minDelay ?? 1000;
  const maxDelay = options.maxDelay ?? 30_000;
  const epoch = api.epoch;
  let stopped = false;
  let socket: WebSocket | null = null;
  let timer: ReturnType<typeof setTimeout> | null = null;
  let failures = 0;
  let attempts = 0;
  const live = () => !stopped && api.epoch === epoch;
  const report = (state: SocketState) => {
    if (live()) handlers.onState?.(state);
  };
  const stop = (final?: SocketState) => {
    if (stopped) return;
    stopped = true;
    unsubscribe();
    if (timer) clearTimeout(timer);
    const open = socket;
    socket = null;
    if (open && open.readyState <= 1) open.close(1000);
    if (final) handlers.onState?.(final);
  };
  const unsubscribe = api.onEpochChange(() => stop({ state: "closed", reason: "session" }));
  // A refused handshake shows only as 1006: one look at the session (a 401
  // there changes the epoch, which stops this socket).
  const probe = async () => {
    try {
      await api.get("/auth/me");
    } catch {
      // The epoch, if anything, tells.
    }
  };
  const connect = async () => {
    if (!live()) return;
    attempts += 1;
    let token: string;
    try {
      // A reconnect reads the session's current token, not the cached one.
      if (attempts > 1) api.refreshCsrf();
      token = await api.csrfToken();
    } catch {
      return stop({ state: "closed", reason: "session" });
    }
    if (!live()) return;
    const ws = new Socket(url, ["rustcms.v1", `rustcms.csrf.${token}`]);
    ws.binaryType = "arraybuffer";
    socket = ws;
    let openedAt: number | null = null;
    ws.onopen = () => {
      if (!live() || socket !== ws) return ws.close(1000);
      openedAt = Date.now();
      report({ state: "open" });
      handlers.onOpen?.();
    };
    ws.onmessage = (event: MessageEvent) => {
      if (live() && socket === ws) handlers.onMessage(event.data as string | ArrayBuffer);
    };
    ws.onclose = (event: CloseEvent) => {
      if (socket !== ws) return;
      socket = null;
      if (!live()) return;
      const detail = { code: event.code, reason: event.reason || undefined };
      if (FINAL_CODES.has(event.code)) {
        if (event.code === 4401) void probe();
        return stop({ state: "closed", ...detail });
      }
      if (!options.reconnect) return stop({ state: "closed", ...detail });
      const healthy = openedAt !== null && Date.now() - openedAt >= HEALTHY_MS;
      failures = healthy ? 0 : failures + 1;
      if (failures >= MAX_FAILURES) return stop({ state: "closed", ...detail });
      report({ state: "reconnecting", ...detail });
      const delay = Math.min(maxDelay, minDelay * 2 ** Math.max(0, failures - 1)) * (0.5 + Math.random() / 2);
      const wait = openedAt === null ? probe() : Promise.resolve();
      void wait.then(() => {
        if (!live()) return;
        timer = setTimeout(() => {
          timer = null;
          void connect();
        }, delay);
      });
    };
  };
  void connect();
  return {
    send: (data) => {
      const ws = socket;
      if (!live() || !ws || ws.readyState !== 1) return false;
      const size = byteLength(data);
      if (size > maxBytes || ws.bufferedAmount + size > MAX_BUFFERED) return false;
      ws.send(data);
      return true;
    },
    close: () => stop(),
  };
}
