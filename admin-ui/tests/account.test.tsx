import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { RouterProvider, createMemoryHistory } from "@tanstack/react-router";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { http, HttpResponse } from "msw";
import { setupServer } from "msw/node";
import { SessionContext } from "@/api/context";
import { createAppRouter } from "@/app/router";
import { createSession } from "@/app/session";

let slowRelease: () => void = () => {};
const server = setupServer(
  http.get("*/admin/api/auth/me", () => HttpResponse.json({ user: { id: "1" }, guard: "admin", permissions: [], superAdmin: false })),
  http.get("*/admin/api/auth/csrf", () => HttpResponse.json({ csrfToken: "t", userId: "1" })),
  http.get("*/admin/api/panel", () =>
    HttpResponse.json({ brand: "T", user: { id: "1", name: "Me", email: "me@x.y" }, navigation: [], system: { users: false, roles: false } }),
  ),
  http.post("*/admin/api/auth/logout-others", () =>
    HttpResponse.json({ message: "x", code: "validation_failed", errors: { current_password: ["The password is incorrect."] } }, { status: 422 }),
  ),
  http.post("*/admin/api/auth/password", () => new HttpResponse(null, { status: 204 })),
  http.get("*/admin/api/slow", async () => {
    await new Promise<void>((r) => (slowRelease = r));
    return HttpResponse.json({ message: "Unauthenticated.", code: "unauthenticated" }, { status: 401 });
  }),
);
beforeAll(() => server.listen({ onUnhandledRequest: "error" }));
afterAll(() => server.close());
afterEach(() => server.resetHandlers());

function mount() {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const toLogin = vi.fn();
  const session = createSession({ basePath: "", apiPath: "http://localhost/admin/api" }, queryClient, toLogin);
  const router = createAppRouter({ queryClient, session }, createMemoryHistory({ initialEntries: ["/account"] }));
  render(
    <SessionContext.Provider value={session}>
      <QueryClientProvider client={queryClient}>
        <RouterProvider router={router} />
      </QueryClientProvider>
    </SessionContext.Provider>,
  );
  return { session, toLogin };
}

it("shows a wrong password next to the form that sent it", async () => {
  mount();
  await userEvent.type(await screen.findByLabelText("Password"), "wrong");
  await userEvent.click(screen.getByRole("button", { name: "Sign out other sessions" }));
  const error = await screen.findByText("The password is incorrect.");
  // Inside the "Other sessions" form, not under "Current password".
  expect(error.closest("form")).toBe(screen.getByRole("button", { name: "Sign out other sessions" }).closest("form"));
  expect(screen.getByLabelText("Password")).toHaveAttribute("aria-invalid", "true");
  expect(screen.getByLabelText("Password")).toHaveAccessibleDescription(/The password is incorrect/);
  expect(screen.getByLabelText("Current password")).not.toHaveAttribute("aria-invalid", "true");
});

it("shows the existing identity beside a labelled security section", async () => {
  mount();
  expect(await screen.findByText("me@x.y", { selector: "dd" })).toBeInTheDocument();
  expect(screen.getByRole("heading", { name: "Profile" })).toBeInTheDocument();
  expect(screen.getByRole("region", { name: "Security" })).toContainElement(screen.getByLabelText("Current password"));
});

it("connects password validation errors to their matching input", async () => {
  server.use(http.post("*/admin/api/auth/password", () =>
    HttpResponse.json({ message: "x", code: "validation_failed", errors: { password: ["Choose a stronger password."] } }, { status: 422 }),
  ));
  mount();
  await userEvent.type(await screen.findByLabelText("New password", { selector: "#new-password" }), "short");
  await userEvent.click(screen.getByRole("button", { name: "Change password" }));
  await screen.findByText("Choose a stronger password.");
  expect(screen.getByLabelText("New password", { selector: "#new-password" })).toHaveAccessibleDescription("Choose a stronger password.");
  expect(screen.getByLabelText("New password", { selector: "#new-password" })).toHaveAttribute("aria-invalid", "true");
  expect(screen.getByLabelText("Current password")).not.toHaveAttribute("aria-invalid", "true");
});

it("shows form-level validation messages", async () => {
  server.use(http.post("*/admin/api/auth/password", () =>
    HttpResponse.json({ message: "The given data was invalid.", code: "validation_failed", errors: {}, formErrors: ["Your account changed meanwhile."] }, { status: 422 }),
  ));
  mount();
  await userEvent.type(await screen.findByLabelText("New password", { selector: "#new-password" }), "long enough");
  await userEvent.click(screen.getByRole("button", { name: "Change password" }));
  expect(await screen.findByText("Your account changed meanwhile.")).toBeInTheDocument();
});

it("pauses account changes after one with an unknown outcome", async () => {
  server.use(http.post("*/admin/api/auth/password", () => new HttpResponse(null, { status: 504 })));
  mount();
  await userEvent.type(await screen.findByLabelText("New password", { selector: "#new-password" }), "long enough");
  await userEvent.click(screen.getByRole("button", { name: "Change password" }));
  expect(await screen.findByText(/Actions are paused/)).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Change password" })).toBeDisabled();
});

it("a 401 for a request sent before a password change does not sign the tab out", async () => {
  const { session, toLogin } = mount();
  await screen.findByLabelText("Current password");
  const stale = session.api.get("/slow").catch(() => undefined);
  await new Promise((r) => setTimeout(r, 10));
  await userEvent.type(screen.getByLabelText("Current password"), "old password");
  await userEvent.type(screen.getByLabelText("New password", { selector: "#new-password" }), "a new password");
  await userEvent.type(screen.getByLabelText("Confirm new password"), "a new password");
  await userEvent.click(screen.getByRole("button", { name: "Change password" }));
  expect(await screen.findByText(/Password changed/)).toBeInTheDocument();
  slowRelease();
  await stale;
  await new Promise((r) => setTimeout(r, 20));
  expect(toLogin).not.toHaveBeenCalled();
  await waitFor(() => expect(screen.getByText(/Password changed/)).toBeInTheDocument());
});

it("a 401 racing with an in-flight password change does not sign the tab out", async () => {
  let finish: () => void = () => {};
  server.use(
    http.post("*/admin/api/auth/password", async () => {
      await new Promise<void>((r) => (finish = r));
      return new HttpResponse(null, { status: 204 });
    }),
    http.get("*/admin/api/racing", () =>
      HttpResponse.json({ message: "Unauthenticated.", code: "unauthenticated" }, { status: 401 }),
    ),
  );
  const { session, toLogin } = mount();
  await userEvent.type(await screen.findByLabelText("Current password"), "old password");
  await userEvent.type(screen.getByLabelText("New password", { selector: "#new-password" }), "a new password");
  await userEvent.type(screen.getByLabelText("Confirm new password"), "a new password");
  await userEvent.click(screen.getByRole("button", { name: "Change password" }));
  await new Promise((r) => setTimeout(r, 20));
  // Sent while the change is in flight (same epoch), answered 401 before it completes.
  await session.api.get("/racing").catch(() => undefined);
  expect(toLogin).not.toHaveBeenCalled();
  finish();
  expect(await screen.findByText(/Password changed/)).toBeInTheDocument();
  expect(toLogin).not.toHaveBeenCalled();
  server.resetHandlers();
});
