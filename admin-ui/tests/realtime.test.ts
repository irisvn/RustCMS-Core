import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { ApiClient } from "@/api/client";
import { openEvents } from "@/lib/realtime";

/** A response whose body the test writes to. */
function streamed(status = 200) {
  let controller!: ReadableStreamDefaultController<Uint8Array>;
  const body = new ReadableStream<Uint8Array>({
    start(c) {
      controller = c;
    },
  });
  const encoder = new TextEncoder();
  return {
    response: new Response(status === 200 ? body : null, { status }),
    send: (text: string) => controller.enqueue(encoder.encode(text)),
    end: () => controller.close(),
  };
}

function client(responses: Array<() => Response>) {
  const calls: Array<{ url: string; signal?: AbortSignal | null }> = [];
  const fetch = vi.fn(async (url: RequestInfo | URL, init?: RequestInit) => {
    calls.push({ url: String(url), signal: init?.signal });
    const next = responses.shift();
    if (!next) throw new TypeError("network down");
    return next();
  });
  return { api: new ApiClient({ apiPath: "/admin/api", fetch: fetch as typeof globalThis.fetch }), fetch, calls };
}

describe("openEvents", () => {
  beforeEach(() => vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout"] }));
  afterEach(() => vi.useRealTimers());

  it("delivers parsed events and stops for good on closed: forbidden", async () => {
    const first = streamed();
    const { api, fetch, calls } = client([() => first.response]);
    const events: unknown[] = [];
    const states: unknown[] = [];
    openEvents(api, "/docker/stats/web", { onEvent: (n, d) => events.push([n, d]), onState: (s) => states.push(s) });
    await vi.waitFor(() => expect(fetch).toHaveBeenCalledTimes(1));
    expect(calls[0]!.url).toBe("/admin/api/x/docker/stats/web");
    first.send('event: tick\ndata: {"n":1}\n\n: keep-alive\n\nevent: tick\r\ndata: {"n":2}\r\n\r\n');
    await vi.waitFor(() => expect(events).toEqual([["tick", { n: 1 }], ["tick", { n: 2 }]]));
    first.send('event: closed\ndata: {"reason":"forbidden"}\n\n');
    first.end();
    await vi.waitFor(() => expect(states.at(-1)).toEqual({ state: "closed", reason: "forbidden" }));
    await vi.advanceTimersByTimeAsync(60_000);
    expect(fetch).toHaveBeenCalledTimes(1);
  });

  it("stops on 401 and 403, retries 429 and network drops with backoff", async () => {
    const refused = client([() => new Response(null, { status: 401 })]);
    const states: unknown[] = [];
    openEvents(refused.api, "/a", { onEvent: () => {}, onState: (s) => states.push(s) });
    await vi.waitFor(() => expect(states).toEqual([{ state: "closed", status: 401 }]));
    await vi.advanceTimersByTimeAsync(60_000);
    expect(refused.fetch).toHaveBeenCalledTimes(1);

    const later = streamed();
    const busy = client([() => new Response(null, { status: 429 }), () => later.response]);
    openEvents(busy.api, "/a", { onEvent: () => {} }, { minDelay: 1000 });
    await vi.waitFor(() => expect(busy.fetch).toHaveBeenCalledTimes(1));
    // Jittered: the first retry comes between half the delay and the delay.
    await vi.advanceTimersByTimeAsync(499);
    expect(busy.fetch).toHaveBeenCalledTimes(1);
    await vi.advanceTimersByTimeAsync(501);
    expect(busy.fetch).toHaveBeenCalledTimes(2);

    // A stream ended without `closed` (a deploy, a dropped connection) reconnects.
    const dropped = streamed();
    const again = streamed();
    const drop = client([() => dropped.response, () => again.response]);
    openEvents(drop.api, "/a", { onEvent: () => {} });
    await vi.waitFor(() => expect(drop.fetch).toHaveBeenCalledTimes(1));
    dropped.end();
    await vi.advanceTimersByTimeAsync(1000);
    expect(drop.fetch).toHaveBeenCalledTimes(2);
  });

  it("closes at once when the session changes, and on close()", async () => {
    const open = streamed();
    const { api, fetch, calls } = client([() => open.response]);
    const events: unknown[] = [];
    const states: unknown[] = [];
    openEvents(api, "/a", { onEvent: (n) => events.push(n), onState: (s) => states.push(s) });
    await vi.waitFor(() => expect(fetch).toHaveBeenCalledTimes(1));
    api.resetCsrf(); // signed out, signed in, rotated: a new epoch
    expect(calls[0]!.signal?.aborted).toBe(true);
    expect(states.at(-1)).toEqual({ state: "closed", reason: "session" });
    await vi.advanceTimersByTimeAsync(60_000);
    expect(fetch).toHaveBeenCalledTimes(1);
    expect(events).toEqual([]);

    const pending = client([() => new Response(null, { status: 503 })]);
    const handle = openEvents(pending.api, "/a", { onEvent: () => {} });
    await vi.waitFor(() => expect(pending.fetch).toHaveBeenCalledTimes(1));
    handle.close();
    await vi.advanceTimersByTimeAsync(60_000);
    expect(pending.fetch).toHaveBeenCalledTimes(1);
  });

  it("refuses paths outside the namespace", () => {
    const { api } = client([]);
    expect(() => openEvents(api, "docker", { onEvent: () => {} })).toThrow();
    expect(() => openEvents(api, "/../auth/me", { onEvent: () => {} })).toThrow();
  });
});
