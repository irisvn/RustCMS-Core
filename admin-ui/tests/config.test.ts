import { readConfig, safeHref, safeRedirect } from "@/lib/config";

describe("runtime config", () => {
  it("reads the meta tag and normalizes the base path", () => {
    document.head.innerHTML = `<meta name="rustcms-admin" content='{"basePath":"/back-office/"}'>`;
    expect(readConfig()).toEqual({ basePath: "/back-office", apiPath: "/back-office/api" });
    document.head.innerHTML = `<meta name="rustcms-admin" content="not json">`;
    expect(readConfig().basePath).toBe("/admin");
    document.head.innerHTML = "";
    expect(readConfig().basePath).toBe("/admin");
  });
});

describe("safeRedirect", () => {
  it("accepts paths inside the panel", () => {
    expect(safeRedirect("/admin/r/posts?page=2", "/admin")).toBe("/admin/r/posts?page=2");
    expect(safeRedirect("/admin", "/admin")).toBe("/admin");
  });

  it.each([
    "//evil.example/admin",
    "/\\evil.example",
    "\\\\evil.example",
    "https://evil.example/admin",
    "javascript:alert(1)",
    "/administrator",
    "/admin/api/auth/logout",
    "/other",
    "/admin/../other",
    "%2F%2Fevil.example",
    "",
    null,
  ])("rejects %s", (target) => {
    expect(safeRedirect(target, "/admin")).toBeNull();
  });
});

describe("safeHref", () => {
  it("only allows web, mail and relative links", () => {
    expect(safeHref("https://example.com/x")).toBe("https://example.com/x");
    expect(safeHref("mailto:a@b.c")).toBe("mailto:a@b.c");
    expect(safeHref("/admin/r/posts")).toBe("/admin/r/posts");
    expect(safeHref("javascript:alert(1)")).toBeNull();
    expect(safeHref("data:text/html,x")).toBeNull();
    expect(safeHref("//evil.example")).toBeNull();
  });
});
