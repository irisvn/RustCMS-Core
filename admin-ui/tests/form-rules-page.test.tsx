// Rules computed on the server on the record page (ADR 0044, part E): a
// change of an input others depend on asks `form/rules` once typing pauses;
// the latest answer is applied (shown, required), also when another input
// changed meanwhile. The schema from the server's contract.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { RouterProvider, createMemoryHistory } from "@tanstack/react-router";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { http, HttpResponse, delay } from "msw";
import { setupServer } from "msw/node";
import { SessionContext } from "@/api/context";
import type { FormRulesRequest, ResourceSchema } from "@/api/types";
import { createAppRouter } from "@/app/router";
import { createSession } from "@/app/session";
import { stayOnPurpose } from "@/lib/leave";
import create from "../../fixtures/contract/form-rules.schema.create.json";

let requests: FormRulesRequest[] = [];
let held: Promise<void> = Promise.resolve();
const vietnam = (request: FormRulesRequest) => request.state.country === "vn";

const server = setupServer(
  http.get("*/admin/api/auth/me", () => HttpResponse.json({ user: { id: "1", name: "A", email: "a@b.c" }, guard: "admin", permissions: [], superAdmin: true })),
  http.get("*/admin/api/auth/csrf", () => HttpResponse.json({ csrfToken: "t", userId: "1" })),
  http.get("*/admin/api/panel", () =>
    HttpResponse.json({ brand: "Test", user: { id: "1", name: "Ada", email: "a@b.c" }, navigation: [(create as unknown as ResourceSchema).resource] }),
  ),
  http.get("*/admin/api/resources/accounts/schema", () => HttpResponse.json(create)),
  http.post("*/admin/api/resources/accounts/form/rules", async ({ request }) => {
    const body = (await request.json()) as FormRulesRequest;
    requests.push(body);
    await held;
    return HttpResponse.json({
      seq: body.seq,
      rules: {
        tax_id: { shown: true, required: vietnam(body) },
        note: { shown: vietnam(body), required: false },
        topic: { shown: true, required: false, options: [{ value: "poetry", label: "POETRY" }] },
      },
    });
  }),
);
beforeAll(() => server.listen({ onUnhandledRequest: "error" }));
afterAll(() => server.close());
beforeEach(() => {
  stayOnPurpose();
  requests = [];
  held = Promise.resolve();
});

function mount(path: string) {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const session = createSession({ basePath: "", apiPath: "http://localhost/admin/api" }, queryClient, () => {});
  const router = createAppRouter({ queryClient, session }, createMemoryHistory({ initialEntries: [path] }));
  render(
    <SessionContext.Provider value={session}>
      <QueryClientProvider client={queryClient}>
        <RouterProvider router={router} />
      </QueryClientProvider>
    </SessionContext.Provider>,
  );
}

it("asks again when an input the rules depend on changes, and applies the answer", async () => {
  mount("/r/accounts/create");
  await screen.findByLabelText(/^Country/);
  // Hidden by its rule when the form opens.
  expect(screen.queryByLabelText(/^Note/)).toBeNull();
  // Typing elsewhere asks nothing.
  await userEvent.type(screen.getByLabelText(/^Name/), "A");
  await delay(500);
  expect(requests).toHaveLength(0);
  await userEvent.type(screen.getByLabelText(/^Country/), "vn");
  expect(await screen.findByLabelText(/^Note/)).toBeInTheDocument();
  expect(requests).toHaveLength(1);
  expect(requests[0]).toMatchObject({ operation: "create", state: { country: "vn" } });
  // Required now: the mark next to its label.
  await waitFor(() => expect(screen.getByText(/^Tax/, { selector: "label" }).textContent).toContain("*"));
});

it("applies the answer when an input it does not depend on changed meanwhile", async () => {
  let release!: () => void;
  held = new Promise<void>((resolve) => (release = resolve));
  mount("/r/accounts/create");
  await userEvent.type(await screen.findByLabelText(/^Country/), "vn");
  await waitFor(() => expect(requests).toHaveLength(1));
  await userEvent.type(screen.getByLabelText(/^Name/), "B");
  release();
  expect(await screen.findByLabelText(/^Note/)).toBeInTheDocument();
});
