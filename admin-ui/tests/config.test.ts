import { readConfig, safeHref, safeRedirect } from "@/lib/config";

describe("runtime config", () => {
  it("reads the meta tag and normalizes the base path", () => {
    document.head.innerHTML = `<meta name="rustcms-admin" content='{"basePath":"/back-office/"}'>`;
    expect(readConfig()).toEqual({ basePath: "/back-office", apiPath: "/back-office/api", theme: { colorScheme: "system" }, signIn: { passwordReset: false, remember: false } });
    document.head.innerHTML = `<meta name="rustcms-admin" content="not json">`;
    expect(readConfig().basePath).toBe("/admin");
    document.head.innerHTML = "";
    expect(readConfig().basePath).toBe("/admin");
  });

  it("keeps only the panel's own extension assets", () => {
    const extensions = {
      scripts: ["assets/ext/acme.0123abcd.js", "https://evil.example/x.js", "assets/ext/acme.0123abcd.css", "assets/ext/../x.0123abcd.js"],
      styles: ["assets/ext/acme.0123abcd.css", "assets/ext/acme.0123abcd.js", 42],
    };
    document.head.innerHTML = `<meta name="rustcms-admin" content='${JSON.stringify({ basePath: "/admin", extensions })}'>`;
    expect(readConfig().extensions).toEqual({ scripts: ["assets/ext/acme.0123abcd.js"], styles: ["assets/ext/acme.0123abcd.css"] });
  });
});

describe("socket message limit", () => {
  const meta = (realtime: unknown) => {
    document.head.innerHTML = `<meta name="rustcms-admin" content='${JSON.stringify({ basePath: "/admin", realtime })}'>`;
    return readConfig().realtime;
  };

  it("keeps an integer the server could have set", () => {
    expect(meta({ maxMessageBytes: 4096 })).toEqual({ maxMessageBytes: 4096 });
    expect(meta({ maxMessageBytes: 1 })).toEqual({ maxMessageBytes: 1 });
    expect(meta({ maxMessageBytes: 1024 * 1024 })).toEqual({ maxMessageBytes: 1024 * 1024 });
  });

  it("ignores anything else (the socket keeps its default)", () => {
    for (const bad of [0, -1, 1024 * 1024 + 1, 1.5, "4096", null, Number.MAX_SAFE_INTEGER]) {
      expect(meta({ maxMessageBytes: bad })).toBeUndefined();
    }
    expect(meta(undefined)).toBeUndefined();
    expect(meta("4096")).toBeUndefined();
    document.head.innerHTML = `<meta name="rustcms-admin" content='{"basePath":"/admin"}'>`;
    expect("realtime" in readConfig()).toBe(false);
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
