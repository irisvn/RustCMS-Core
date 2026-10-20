// Branding in the SPA (ADR 0033): the index configuration's brand, logos and
// theme stylesheet; one branding per document.
import { act, render, screen } from "@testing-library/react";
import { SessionContext } from "@/api/context";
import type { Session } from "@/app/session";
import { BrandHeader, BrandMark } from "@/components/brand";
import { readConfig, type PanelConfig } from "@/lib/config";

const meta = (config: unknown) => {
  document.head.innerHTML = `<meta name="rustcms-admin" content='${JSON.stringify(config)}'>`;
};

describe("readConfig", () => {
  it("keeps the brand, logos and theme stylesheet under assets/brand only", () => {
    meta({
      basePath: "/admin",
      brand: "Acme",
      extensions: { scripts: [], styles: ["assets/brand/theme.css?v=0123abcd", "assets/brand/theme.css?v=zz", "assets/brand/x.css?v=0123abcd", "assets/ext/acme.0123abcd.css"] },
      theme: { colorScheme: "dark", logo: "assets/brand/logo?v=0123abcd", logoDark: "assets/brand/logo-dark?v=89abcdef", favicon: "assets/brand/favicon?v=0123abcd" },
    });
    const config = readConfig();
    expect(config.brand).toBe("Acme");
    expect(config.extensions?.styles).toEqual(["assets/brand/theme.css?v=0123abcd", "assets/ext/acme.0123abcd.css"]);
    expect(config.theme).toEqual({ colorScheme: "dark", logo: "assets/brand/logo?v=0123abcd", logoDark: "assets/brand/logo-dark?v=89abcdef", favicon: "assets/brand/favicon?v=0123abcd" });
  });

  it("keeps a long brand whole: 64 characters, not UTF-16 units", () => {
    const name = "a".repeat(63) + "😀";
    meta({ basePath: "/admin", brand: name + "tail" });
    expect(readConfig().brand).toBe(name);
  });

  it("drops paths outside the allowlist and blank brands", () => {
    meta({
      basePath: "/admin",
      brand: "   ",
      theme: { colorScheme: "light", logo: "https://evil.example/logo.png", logoDark: "assets/brand/logo?v=0123abcd", favicon: "assets/brand/favicon?v=0123abcd&x=1" },
    });
    const config = readConfig();
    expect(config.brand).toBeUndefined();
    expect(config.theme).toEqual({ colorScheme: "light", logo: undefined, logoDark: undefined, favicon: undefined });
  });
});

function withConfig(config: Partial<PanelConfig>, node: React.ReactNode) {
  const session = { config: { basePath: "/admin", apiPath: "/admin/api", ...config } } as unknown as Session;
  return render(<SessionContext.Provider value={session}>{node}</SessionContext.Provider>);
}

describe("brand", () => {
  afterEach(() => {
    delete document.documentElement.dataset.scheme;
  });

  it("shows the configured logo and name above the sign-in form, and titles the tab", () => {
    withConfig({ brand: "Acme", theme: { colorScheme: "system", logo: "assets/brand/logo?v=0123abcd" } }, <BrandHeader />);
    expect(screen.getByText("Acme")).toBeInTheDocument();
    expect(document.querySelector("img")).toHaveAttribute("src", "/admin/assets/brand/logo?v=0123abcd");
    expect(document.title).toBe("Acme");
  });

  it("falls back to the product name and default mark", () => {
    withConfig({}, <BrandHeader />);
    expect(screen.getByText("rustcms")).toBeInTheDocument();
    expect(document.querySelector("img")).toBeNull();
  });

  it("follows the page's scheme for the dark logo", async () => {
    withConfig({ theme: { colorScheme: "system", logo: "assets/brand/logo?v=0123abcd", logoDark: "assets/brand/logo-dark?v=89abcdef" } }, <BrandMark />);
    expect(document.querySelector("img")).toHaveAttribute("src", "/admin/assets/brand/logo?v=0123abcd");
    await act(async () => {
      document.documentElement.dataset.scheme = "dark";
      await Promise.resolve();
    });
    expect(document.querySelector("img")).toHaveAttribute("src", "/admin/assets/brand/logo-dark?v=89abcdef");
  });
});
