import type { ApiClient } from "@/api/client";

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
  if (!path.startsWith("/") || path.split("/").some((s) => s === ".." || s === ".")) {
    throw new Error(`invalid stream path ${path}`);
  }
  const url = `${api.apiPath}/x${path}`;
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
    if (!response.ok || !response.body) return retry({ status: response.status });
    report({ state: "open" });
    const reader = response.body.getReader();
    const decoder = new TextDecoder();
    let buffer = "";
    let reason: string | undefined;
    try {
      for (;;) {
        const { value, done } = await reader.read();
        if (done || !live()) break;
        buffer += decoder.decode(value, { stream: true }).replace(/\r\n?/g, "\n");
        let end: number;
        while ((end = buffer.indexOf("\n\n")) >= 0) {
          const block = buffer.slice(0, end);
          buffer = buffer.slice(end + 2);
          const event = parse(block);
          if (!event || !live()) continue;
          if (event.name === "closed") {
            reason = (event.data as { reason?: string } | null)?.reason ?? "closed";
            continue;
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
