import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { RouterProvider, createMemoryHistory } from "@tanstack/react-router";
import { act, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { http, HttpResponse } from "msw";
import { setupServer } from "msw/node";
import { SessionContext } from "@/api/context";
import { createAppRouter } from "@/app/router";
import { createSession } from "@/app/session";
import { applyTheme, chooseScheme, schemeChoice } from "@/lib/theme";
import { useColorScheme } from "../sdk/index.js";

const config = (colorScheme: "system" | "light" | "dark") => ({ basePath: "/admin", apiPath: "/admin/api", theme: { colorScheme } });
const scheme = () => document.documentElement.dataset.scheme;
let stops: Array<() => void> = [];
const apply = (c: ReturnType<typeof config>) => stops.push(applyTheme(c));

afterEach(() => {
  for (const stop of stops) stop();
  stops = [];
  localStorage.clear();
  vi.restoreAllMocks();
});

describe("color scheme", () => {
  it("the app's default applies until the user chooses, and the choice is kept for this browser", () => {
    apply(config("dark"));
    expect(scheme()).toBe("dark");
    expect(schemeChoice()).toBe("dark");
    chooseScheme("light");
    expect(scheme()).toBe("light");
    // Next visit: the choice wins over the default.
    for (const stop of stops) stop();
    stops = [];
    apply(config("dark"));
    expect(scheme()).toBe("light");
  });

  it("System follows the operating system", () => {
    let dark = true;
    const listeners: Array<() => void> = [];
    vi.spyOn(window, "matchMedia").mockImplementation(
      () => ({ get matches() { return dark; }, addEventListener: (_: string, f: () => void) => listeners.push(f), removeEventListener() {} }) as never,
    );
    apply(config("light"));
    chooseScheme("system");
    expect(scheme()).toBe("dark");
    dark = false;
    for (const f of listeners) f();
    expect(scheme()).toBe("light");
  });

  it("blocked or invalid storage never breaks the panel", () => {
    localStorage.setItem("rustcms.colorScheme", "purple");
    apply(config("dark"));
    expect(scheme()).toBe("dark");
    vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => {
      throw new Error("blocked");
    });
    vi.spyOn(Storage.prototype, "getItem").mockImplementation(() => {
      throw new Error("blocked");
    });
    chooseScheme("light");
    expect(scheme()).toBe("light");
  });

  it("another tab's choice, or cleared storage, applies here (even after a blocked write)", () => {
    apply(config("dark"));
    const set = vi.spyOn(Storage.prototype, "setItem").mockImplementationOnce(() => {
      throw new Error("blocked");
    });
    chooseScheme("light");
    expect(scheme()).toBe("light");
    set.mockRestore();
    localStorage.setItem("rustcms.colorScheme", "dark");
    window.dispatchEvent(new StorageEvent("storage", { key: "rustcms.colorScheme" }));
    expect(scheme()).toBe("dark");
    localStorage.clear();
    window.dispatchEvent(new StorageEvent("storage", { key: null }));
    expect(scheme()).toBe("dark");
    expect(schemeChoice()).toBe("dark");
  });

  it("the SDK hook follows the scheme", () => {
    apply(config("light"));
    function Probe() {
      return <p>scheme {useColorScheme()}</p>;
    }
    render(<Probe />);
    expect(screen.getByText("scheme light")).toBeInTheDocument();
    act(() => chooseScheme("dark"));
    return screen.findByText("scheme dark");
  });
});

const user = { id: "1", name: "Admin", email: "admin@example.test" };
const server = setupServer(
  http.get("*/api/auth/me", () => HttpResponse.json({ user, guard: "admin", permissions: [], superAdmin: false })),
  http.get("*/api/panel", () => HttpResponse.json({ brand: "W", user, navigation: [], system: { users: false, roles: false }, settings: [] })),
  http.get("*/api/dashboard", () => HttpResponse.json({ widgets: [], resourceLinks: true })),
);
beforeAll(() => server.listen({ onUnhandledRequest: "error" }));
afterAll(() => server.close());

it("the sidebar switch chooses Light, Dark or System", async () => {
  apply(config("system"));
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const session = createSession({ basePath: "", apiPath: "http://localhost/api" }, queryClient, () => {});
  const router = createAppRouter({ queryClient, session }, createMemoryHistory({ initialEntries: ["/"] }));
  render(
    <SessionContext.Provider value={session}>
      <QueryClientProvider client={queryClient}>
        <RouterProvider router={router} />
      </QueryClientProvider>
    </SessionContext.Provider>,
  );
  const group = (await screen.findAllByRole("group", { name: "Color scheme" }))[0]!;
  const dark = group.querySelector('[aria-label="Dark"]') as HTMLButtonElement;
  expect(group.querySelector('[aria-label="System"]')).toHaveAttribute("aria-pressed", "true");
  await userEvent.click(dark);
  expect(scheme()).toBe("dark");
  expect(dark).toHaveAttribute("aria-pressed", "true");
  expect(localStorage.getItem("rustcms.colorScheme")).toBe("dark");
});
