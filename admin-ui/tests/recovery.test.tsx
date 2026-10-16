import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { RouterProvider, createMemoryHistory } from "@tanstack/react-router";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { http, HttpResponse } from "msw";
import { setupServer } from "msw/node";
import { SessionContext } from "@/api/context";
import { createAppRouter } from "@/app/router";
import { createSession } from "@/app/session";

const bodies: { path: string; body: unknown }[] = [];
const server = setupServer(
  http.get("*/admin/api/auth/me", () => HttpResponse.json({ message: "Unauthenticated.", code: "unauthenticated" }, { status: 401 })),
  http.get("*/admin/api/auth/csrf", () => HttpResponse.json({ csrfToken: "t", userId: null })),
  http.post("*/admin/api/auth/forgot-password", async ({ request }) => {
    bodies.push({ path: "forgot", body: await request.json() });
    return HttpResponse.json({}, { status: 202 });
  }),
  http.post("*/admin/api/auth/reset-password", async ({ request }) => {
    bodies.push({ path: "reset", body: await request.json() });
    return new HttpResponse(null, { status: 204 });
  }),
  http.post("*/admin/api/auth/login", async ({ request }) => {
    bodies.push({ path: "login", body: await request.json() });
    return HttpResponse.json({ message: "x", code: "validation_failed", errors: { email: ["These credentials do not match our records."] } }, { status: 422 });
  }),
);
beforeAll(() => server.listen({ onUnhandledRequest: "bypass" }));
afterAll(() => server.close());
beforeEach(() => (bodies.length = 0));

function mount(path: string, signIn = { passwordReset: true, remember: true }) {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const session = createSession({ basePath: "", apiPath: "http://localhost/admin/api", signIn }, queryClient, () => {});
  const router = createAppRouter({ queryClient, session }, createMemoryHistory({ initialEntries: [path] }));
  render(
    <SessionContext.Provider value={session}>
      <QueryClientProvider client={queryClient}>
        <RouterProvider router={router} />
      </QueryClientProvider>
    </SessionContext.Provider>,
  );
}

it("asks for a reset link and answers the same whoever asks", async () => {
  mount("/forgot-password");
  await userEvent.type(await screen.findByLabelText("Email"), "ghost@example.test");
  await userEvent.click(screen.getByRole("button", { name: "Send the link" }));
  expect(await screen.findByText(/If this email has an account/)).toBeInTheDocument();
  expect(bodies).toEqual([{ path: "forgot", body: { email: "ghost@example.test" } }]);
});

it("reads the token from the fragment, then removes it from the address bar", async () => {
  const token = "0199d3a4-1b2c-7d3e-8f00-000000000000.AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
  window.history.replaceState(null, "", `/reset-password#token=${token}`);
  mount("/reset-password");
  await waitFor(() => expect(window.location.hash).toBe(""));
  await userEvent.type(await screen.findByLabelText("New password"), "a brand new passphrase");
  await userEvent.type(screen.getByLabelText("Confirm the password"), "a brand new passphrase");
  await userEvent.click(screen.getByRole("button", { name: "Save the password" }));
  await waitFor(() => expect(bodies.map((b) => b.path)).toContain("reset"));
  expect(bodies.find((b) => b.path === "reset")!.body).toEqual({
    token,
    password: "a brand new passphrase",
    password_confirmation: "a brand new passphrase",
  });
});

it("offers remember me and the reset link on the sign-in page when configured", async () => {
  mount("/login");
  await userEvent.type(await screen.findByLabelText("Email"), "a@example.test");
  await userEvent.type(screen.getByLabelText("Password"), "secret");
  await userEvent.click(screen.getByLabelText("Remember me"));
  expect(screen.getByRole("link", { name: "Forgot password?" })).toBeInTheDocument();
  await userEvent.click(screen.getByRole("button", { name: /Sign in/ }));
  await waitFor(() => expect(bodies.map((b) => b.path)).toContain("login"));
  expect(bodies.find((b) => b.path === "login")!.body).toEqual({ email: "a@example.test", password: "secret", remember: true });
});

it("hides both when not configured", async () => {
  mount("/login", { passwordReset: false, remember: false });
  await screen.findByLabelText("Email");
  expect(screen.queryByLabelText("Remember me")).toBeNull();
  expect(screen.queryByRole("link", { name: "Forgot password?" })).toBeNull();
});

it("after a lost answer, says the password may already work instead of offering a retry", async () => {
  server.use(http.post("*/admin/api/auth/reset-password", () => HttpResponse.error()));
  const token = "0199d3a4-1b2c-7d3e-8f00-000000000000.AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
  window.history.replaceState(null, "", `/reset-password#token=${token}`);
  mount("/reset-password");
  await userEvent.type(await screen.findByLabelText("New password"), "a brand new passphrase");
  await userEvent.type(screen.getByLabelText("Confirm the password"), "a brand new passphrase");
  await userEvent.click(screen.getByRole("button", { name: "Save the password" }));
  expect(await screen.findByText(/your new password may already work/)).toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "Save the password" })).toBeNull();
});
