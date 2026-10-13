import { QueryClient } from "@tanstack/react-query";
import { createSession } from "@/app/session";
import { ApiError } from "@/api/client";
import { guardedWrite } from "@/api/queries";

describe("session identity", () => {
  it("drops everything when the signed-in user changes under the tab", () => {
    const assign = vi.fn();
    vi.stubGlobal("location", { ...window.location, assign, pathname: "/admin/r/posts", search: "" });
    const queryClient = new QueryClient();
    queryClient.setQueryData(["records", "posts"], { secret: "A's data" });
    const session = createSession({ basePath: "/admin", apiPath: "/admin/api" }, queryClient, () => {});
    session.identify("1");
    session.identify("1");
    expect(assign).not.toHaveBeenCalled();
    session.identify("2");
    expect(queryClient.getQueryData(["records", "posts"])).toBeUndefined();
    expect(assign).toHaveBeenCalledWith("/admin/");
    vi.unstubAllGlobals();
  });
});

describe("rotation", () => {
  it("re-checks a 401 deferred during a rotation once it settles", async () => {
    const queryClient = new QueryClient();
    const toLogin = vi.fn();
    const session = createSession({ basePath: "/admin", apiPath: "/admin/api" }, queryClient, toLogin);
    const probes: string[] = [];
    session.api.request = (async (method: string, path: string) => {
      probes.push(`${method} ${path}`);
      throw new Error("offline");
    }) as typeof session.api.request;
    await session.rotating(async () => {
      // A request racing with the rotation got 401: held, not acted on.
      (session.api as unknown as { opts: { onUnauthenticated: () => void } }).opts.onUnauthenticated();
      expect(toLogin).not.toHaveBeenCalled();
    });
    expect(probes).toContain("GET /auth/me");
  });
});

describe("re-sendable writes", () => {
  const write = (user: string) => ({ id: "w1", user, method: "POST", path: "/x", body: "{}", key: "k1" });

  it("belong to the user who sent them and never survive a change of session", () => {
    const session = createSession({ basePath: "/admin", apiPath: "/admin/api" }, new QueryClient(), () => {});
    session.identify("1");
    session.markUncertain("posts", write("2"));
    expect(session.isUncertain("posts")).toBe(true);
    expect(session.pendingWrite("posts")).toBeUndefined();

    session.markUncertain("posts", write("1"));
    expect(session.pendingWrite("posts")?.key).toBe("k1");
    // Another id does not lift it; the matching one does.
    session.resolved("posts", "other");
    expect(session.isUncertain("posts")).toBe(true);
    session.markUncertain("tags", write("1"));
    session.signedOut({ broadcast: false });
    // A new session starts clean: nothing paused, nothing to re-send.
    expect(session.isUncertain("tags")).toBe(false);
    expect(session.pendingWrite("tags")).toBeUndefined();
  });

  it("lift the pause once resolved", () => {
    const session = createSession({ basePath: "/admin", apiPath: "/admin/api" }, new QueryClient(), () => {});
    session.identify("1");
    session.markUncertain("posts", write("1"));
    session.resolved("posts", "w1");
    expect(session.isUncertain("posts")).toBe(false);
    expect(session.pendingWrite("posts")).toBeUndefined();
  });
});

describe("unknown outcomes across sessions", () => {
  it("an answer arriving after a change of user does not pause the new session", async () => {
    const session = createSession({ basePath: "/admin", apiPath: "/admin/api" }, new QueryClient(), () => {});
    session.identify("1");
    let fail: (e: unknown) => void = () => {};
    const write = guardedWrite(session, "posts", () => new Promise((_, reject) => (fail = reject)));
    await Promise.resolve();
    await Promise.resolve();
    session.signedOut({ broadcast: false });
    session.identify("2");
    fail(new ApiError(504, { code: "outcome_unknown", message: "x" }));
    await expect(write).rejects.toBeInstanceOf(ApiError);
    expect(session.isUncertain("posts")).toBe(false);
  });
});
