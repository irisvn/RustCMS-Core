import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { ApiClient } from "@/api/client";
import { openSocket } from "@/lib/realtime";

/** A WebSocket the test drives. */
class FakeSocket {
  static all: FakeSocket[] = [];
  readyState = 0;
  bufferedAmount = 0;
  binaryType = "blob";
  sent: unknown[] = [];
  closedWith: number | null = null;
  onopen: (() => void) | null = null;
  onmessage: ((e: { data: unknown }) => void) | null = null;
  onclose: ((e: { code: number; reason: string }) => void) | null = null;
  constructor(
    public url: string,
    public protocols: string[],
  ) {
    FakeSocket.all.push(this);
  }
  send(data: unknown) {
    this.sent.push(data);
  }
  close(code: number) {
    this.closedWith = code;
    this.readyState = 3;
  }
  open() {
    this.readyState = 1;
    this.onopen?.();
  }
  drop(code: number, reason = "") {
    this.readyState = 3;
    this.onclose?.({ code, reason });
  }
}

/** A client whose `/auth/csrf` hands out `csrf-1`, `csrf-2`…; `/auth/me` answers `me`. */
function client(me: () => Response = () => Response.json({ id: "1" })) {
  let n = 0;
  const fetch = vi.fn(async (url: RequestInfo | URL) => {
    if (String(url).endsWith("/auth/csrf")) return Response.json({ csrfToken: `csrf-${++n}` });
    if (String(url).endsWith("/auth/me")) return me();
    throw new TypeError("unexpected");
  });
  const api = new ApiClient({ apiPath: "/admin/api", fetch: fetch as typeof globalThis.fetch, onUnauthenticated: () => api.resetCsrf() });
  return { api, fetch };
}

const options = { WebSocket: FakeSocket as unknown as typeof WebSocket };
const last = () => FakeSocket.all.at(-1)!;

describe("openSocket", () => {
  beforeEach(() => {
    FakeSocket.all = [];
    vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout", "Date"] });
  });
  afterEach(() => vi.useRealTimers());

  it("sends the CSRF subprotocol and messages both ways", async () => {
    const { api } = client();
    const got: unknown[] = [];
    const states: unknown[] = [];
    const socket = openSocket(api, "/term/web", { onMessage: (d) => got.push(d), onState: (s) => states.push(s) }, options);
    await vi.waitFor(() => expect(FakeSocket.all).toHaveLength(1));
    expect(last().url).toBe("ws://localhost:3000/admin/api/x/term/web");
    expect(last().protocols).toEqual(["rustcms.v1", "rustcms.csrf.csrf-1"]);
    expect(last().binaryType).toBe("arraybuffer");
    expect(socket.send("early")).toBe(false);
    last().open();
    expect(states).toEqual([{ state: "open" }]);
    expect(socket.send("ls\n")).toBe(true);
    last().onmessage?.({ data: "out" });
    expect(last().sent).toEqual(["ls\n"]);
    expect(got).toEqual(["out"]);
    socket.close();
    expect(last().closedWith).toBe(1000);
  });

  it("refuses a message too large or one that would queue past 1 MiB", async () => {
    const { api } = client();
    const socket = openSocket(api, "/t", { onMessage: () => {} }, { ...options, maxMessageBytes: 10 });
    await vi.waitFor(() => expect(FakeSocket.all).toHaveLength(1));
    last().open();
    expect(socket.send("x".repeat(11))).toBe(false);
    expect(socket.send("é".repeat(6))).toBe(false); // 12 bytes
    expect(socket.send(new Uint8Array(10))).toBe(true);
    last().bufferedAmount = 1024 * 1024 - 5;
    expect(socket.send("123456")).toBe(false);
    expect(socket.send("12345")).toBe(true);
  });

  it("does not reconnect by default, and never after 4401 or 4403", async () => {
    const { api } = client();
    const states: unknown[] = [];
    openSocket(api, "/t", { onMessage: () => {}, onState: (s) => states.push(s) }, options);
    await vi.waitFor(() => expect(FakeSocket.all).toHaveLength(1));
    last().open();
    last().drop(1006);
    expect(states.at(-1)).toEqual({ state: "closed", code: 1006 });
    const forbidden = client();
    const seen: unknown[] = [];
    openSocket(forbidden.api, "/t", { onMessage: () => {}, onState: (s) => seen.push(s) }, { ...options, reconnect: true });
    await vi.waitFor(() => expect(FakeSocket.all).toHaveLength(2));
    last().open();
    last().drop(4403, "forbidden");
    expect(seen.at(-1)).toEqual({ state: "closed", code: 4403, reason: "forbidden" });
    await vi.advanceTimersByTimeAsync(120_000);
    expect(FakeSocket.all).toHaveLength(2);
  });

  it("reconnects with a refreshed token, counting short connections, up to 5", async () => {
    const { api, fetch } = client();
    const states: Array<{ state: string }> = [];
    openSocket(api, "/t", { onMessage: () => {}, onState: (s) => states.push(s) }, { ...options, reconnect: true });
    for (let n = 1; n <= 5; n++) {
      await vi.waitFor(() => expect(FakeSocket.all).toHaveLength(n));
      expect(last().protocols[1]).toBe(`rustcms.csrf.csrf-${n}`);
      // Open briefly, then dropped: still a failure.
      last().open();
      await vi.advanceTimersByTimeAsync(1000);
      last().drop(1006);
      await vi.advanceTimersByTimeAsync(60_000);
    }
    expect(states.at(-1)).toEqual({ state: "closed", code: 1006 });
    expect(FakeSocket.all).toHaveLength(5);
    // Each one opened: the session was never probed.
    expect(fetch.mock.calls.filter(([u]) => String(u).endsWith("/auth/me"))).toHaveLength(0);
  });

  it("a connection open 30 s resets the failures; a refused one probes the session once", async () => {
    let signedIn = true;
    const { api, fetch } = client(() => (signedIn ? Response.json({ id: "1" }) : new Response(null, { status: 401 })));
    const states: Array<{ state: string }> = [];
    openSocket(api, "/t", { onMessage: () => {}, onState: (s) => states.push(s) }, { ...options, reconnect: true });
    await vi.waitFor(() => expect(FakeSocket.all).toHaveLength(1));
    last().open();
    await vi.advanceTimersByTimeAsync(31_000);
    last().drop(1001, "shutdown");
    expect(states.at(-1)).toEqual({ state: "reconnecting", code: 1001, reason: "shutdown" });
    await vi.advanceTimersByTimeAsync(60_000);
    expect(FakeSocket.all).toHaveLength(2);
    // Refused before opening, and the session is gone: the epoch ends it.
    signedIn = false;
    last().drop(1006);
    await vi.waitFor(() => expect(states.at(-1)).toEqual({ state: "closed", reason: "session" }));
    expect(fetch.mock.calls.filter(([u]) => String(u).endsWith("/auth/me"))).toHaveLength(1);
    await vi.advanceTimersByTimeAsync(120_000);
    expect(FakeSocket.all).toHaveLength(2);
  });

  it("closes when the session changes, even while its token is on the way", async () => {
    const { api } = client();
    const states: unknown[] = [];
    openSocket(api, "/t", { onMessage: () => {}, onState: (s) => states.push(s) }, options);
    api.resetCsrf();
    await vi.advanceTimersByTimeAsync(1000);
    expect(FakeSocket.all).toHaveLength(0);
    expect(states).toEqual([{ state: "closed", reason: "session" }]);
    const second = client();
    const got: unknown[] = [];
    openSocket(second.api, "/t", { onMessage: (d) => got.push(d) }, options);
    await vi.waitFor(() => expect(FakeSocket.all).toHaveLength(1));
    last().open();
    second.api.resetCsrf();
    expect(last().closedWith).toBe(1000);
    last().onmessage?.({ data: "late" });
    expect(got).toEqual([]);
  });
});
