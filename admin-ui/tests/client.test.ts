import { http, HttpResponse } from "msw";
import { setupServer } from "msw/node";
import { ApiClient, ApiError } from "@/api/client";

const calls: { method: string; path: string; csrf: string | null }[] = [];
let csrfToken = "t1";
let meStatus = 200;

const server = setupServer(
  http.all("http://localhost/admin/api/*", ({ request }) => {
    const url = new URL(request.url);
    calls.push({ method: request.method, path: url.pathname, csrf: request.headers.get("x-csrf-token") });
    if (url.pathname.endsWith("/auth/csrf")) return HttpResponse.json({ csrfToken });
    if (url.pathname.endsWith("/auth/me")) {
      return meStatus === 200 ? HttpResponse.json({ user: {} }) : HttpResponse.json({ message: "Unauthenticated.", code: "unauthenticated" }, { status: 401 });
    }
    if (url.pathname.endsWith("/stale")) {
      return HttpResponse.json({ message: "CSRF token mismatch.", code: "csrf_mismatch" }, { status: 403 });
    }
    if (url.pathname.endsWith("/slow")) return new HttpResponse(null, { status: 408 });
    if (url.pathname.endsWith("/gateway")) {
      return HttpResponse.json({ message: "Server error.", code: "internal_error" }, { status: 504 });
    }
    if (url.pathname.endsWith("/busy")) {
      return HttpResponse.json({ message: "x", code: "too_many_requests" }, { status: 429, headers: { "Retry-After": "30" } });
    }
    if (url.pathname.endsWith("/invalid")) {
      return HttpResponse.json({ message: "The given data was invalid.", code: "validation_failed", errors: { title: ["is required"] }, formErrors: ["conflict"] }, { status: 422 });
    }
    if (url.pathname.endsWith("/gone")) return HttpResponse.json({ message: "Unauthenticated.", code: "unauthenticated" }, { status: 401 });
    if (url.pathname.endsWith("/down")) return HttpResponse.error();
    if (url.pathname.endsWith("/truncated")) {
      return new HttpResponse('{"data": {"key"', { status: 201, headers: { "Content-Type": "application/json" } });
    }
    return request.method === "DELETE" ? new HttpResponse(null, { status: 204 }) : HttpResponse.json({ ok: true });
  }),
);

beforeAll(() => server.listen({ onUnhandledRequest: "error" }));
afterAll(() => server.close());
beforeEach(() => {
  calls.length = 0;
  csrfToken = "t1";
  meStatus = 200;
});

const client = (onUnauthenticated = vi.fn()) => new ApiClient({ apiPath: "http://localhost/admin/api", onUnauthenticated });

describe("ApiClient", () => {
  it("fetches the CSRF token once and sends it on writes only", async () => {
    const api = client();
    await api.get("/panel");
    await Promise.all([api.post("/a", {}), api.patch("/b", {})]);
    await api.delete("/c");
    expect(calls.filter((c) => c.path.endsWith("/auth/csrf"))).toHaveLength(1);
    expect(calls.find((c) => c.path.endsWith("/panel"))?.csrf).toBeNull();
    expect(calls.filter((c) => ["POST", "PATCH", "DELETE"].includes(c.method)).every((c) => c.csrf === "t1")).toBe(true);
  });

  it("refetches the token after a reset (login/logout rotate it)", async () => {
    const api = client();
    await api.post("/a", {});
    api.resetCsrf();
    csrfToken = "t2";
    await api.post("/a", {});
    expect(calls.at(-1)?.csrf).toBe("t2");
  });

  it("reports 401 once per response and forgets the token", async () => {
    const onUnauthenticated = vi.fn();
    const api = client(onUnauthenticated);
    await expect(api.get("/gone")).rejects.toMatchObject({ status: 401, code: "unauthenticated" });
    expect(onUnauthenticated).toHaveBeenCalledTimes(1);
  });

  it("on a stale CSRF token probes the session and never replays the write", async () => {
    const onUnauthenticated = vi.fn();
    const api = client(onUnauthenticated);
    meStatus = 401;
    const error = await api.post("/stale", {}).catch((e: unknown) => e);
    expect(error).toBeInstanceOf(ApiError);
    expect((error as ApiError).code).toBe("csrf_mismatch");
    expect(calls.filter((c) => c.path.endsWith("/stale"))).toHaveLength(1);
    expect(calls.some((c) => c.path.endsWith("/auth/me"))).toBe(true);
    expect(onUnauthenticated).toHaveBeenCalledTimes(1);
  });

  it("hands the probed identity to the session after a CSRF failure", async () => {
    const onSessionProbe = vi.fn();
    const api = new ApiClient({ apiPath: "http://localhost/admin/api", onSessionProbe });
    await api.post("/stale", {}).catch(() => undefined);
    expect(onSessionProbe).toHaveBeenCalledWith({ user: {} });
  });

  it("maps validation, throttling, empty and network errors", async () => {
    const api = client();
    const invalid = (await api.post("/invalid", {}).catch((e: unknown) => e)) as ApiError;
    expect(invalid.fieldErrors).toEqual({ title: ["is required"] });
    expect(invalid.formErrors).toEqual(["conflict"]);
    const busy = (await api.get("/busy").catch((e: unknown) => e)) as ApiError;
    expect(busy.message).toContain("30 s");
    const slow = (await api.get("/slow").catch((e: unknown) => e)) as ApiError;
    expect(slow.code).toBe("timeout");
    const down = (await api.post("/down", {}).catch((e: unknown) => e)) as ApiError;
    expect(down.code).toBe("network_error");
    expect(down.message).toContain("may have been applied");
  });

  it("reports a cut-off success body of a write as possibly applied", async () => {
    const error = (await client().post("/truncated", {}).catch((e: unknown) => e)) as ApiError;
    expect(error).toBeInstanceOf(ApiError);
    expect(error.code).toBe("incomplete_response");
    expect(error.message).toContain("may have been applied");
  });

  it("never sends a write prepared before the session was reset", async () => {
    let release: () => void = () => {};
    server.use(
      http.get("http://localhost/admin/api/auth/csrf", async () => {
        await new Promise<void>((r) => (release = r));
        return HttpResponse.json({ csrfToken: "B's token", userId: null });
      }),
    );
    const api = client();
    const write = api.post("/a", { draft: "A" }).catch((e: unknown) => e);
    await new Promise((r) => setTimeout(r, 10));
    api.resetCsrf(); // e.g. a 401 on another request
    release();
    const error = (await write) as ApiError;
    expect(error.code).toBe("session_changed");
    expect(calls.some((c) => c.path.endsWith("/a"))).toBe(false);
    server.resetHandlers();
  });

  it("ignores a 401 that answers a request from before the current sign-in", async () => {
    let release: () => void = () => {};
    server.use(
      http.get("http://localhost/admin/api/old", async () => {
        await new Promise<void>((r) => (release = r));
        return HttpResponse.json({ message: "Unauthenticated.", code: "unauthenticated" }, { status: 401 });
      }),
    );
    const onUnauthenticated = vi.fn();
    const api = client(onUnauthenticated);
    const old = api.get("/old").catch((e: unknown) => e);
    await new Promise((r) => setTimeout(r, 10));
    api.resetCsrf(); // signed in again meanwhile
    release();
    expect(((await old) as ApiError).code).toBe("session_changed");
    expect(onUnauthenticated).not.toHaveBeenCalled();
    server.resetHandlers();
  });

  it("treats a failed write's 500 as possibly applied", async () => {
    server.use(http.post("http://localhost/admin/api/boom", () => HttpResponse.json({ message: "Server error.", code: "internal_error" }, { status: 500 })));
    const error = (await client().post("/boom", {}).catch((e: unknown) => e)) as ApiError;
    expect(error.maybeApplied).toBe(true);
    server.resetHandlers();
  });

  it("treats a write timeout as possibly applied, a read timeout as a plain error", async () => {
    const write = (await client().post("/gateway", {}).catch((e: unknown) => e)) as ApiError;
    expect(write.code).toBe("outcome_unknown");
    expect(write.maybeApplied).toBe(true);
    const read = (await client().get("/gateway").catch((e: unknown) => e)) as ApiError;
    expect(read.maybeApplied).toBe(false);
  });

  it("a cut-off read (e.g. the CSRF token) is not a possibly applied write", async () => {
    server.use(
      http.get("http://localhost/admin/api/auth/csrf", () =>
        new HttpResponse('{"csrfToken": "t', { status: 200, headers: { "Content-Type": "application/json" } }),
      ),
    );
    const error = (await client().post("/never-sent", {}).catch((e: unknown) => e)) as ApiError;
    expect(error.code).toBe("incomplete_read");
    expect(error.maybeApplied).toBe(false);
    expect(calls.some((c) => c.path.endsWith("/never-sent"))).toBe(false);
    server.resetHandlers();
  });

  it("returns undefined for 204", async () => {
    await expect(client().delete("/x")).resolves.toBeUndefined();
  });
});
