import { QueryClient } from "@tanstack/react-query";
import { createSession } from "@/app/session";

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
