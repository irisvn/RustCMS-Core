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
