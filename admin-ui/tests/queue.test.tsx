import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { RouterProvider, createMemoryHistory } from "@tanstack/react-router";
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { http, HttpResponse } from "msw";
import { setupServer } from "msw/node";
import { SessionContext } from "@/api/context";
import type { QueueJob, QueueOverview } from "@/api/types";
import { createAppRouter } from "@/app/router";
import { createSession } from "@/app/session";

const job = (id: string, over: Partial<QueueJob> = {}): QueueJob => ({
  id,
  queue: "default",
  kind: "lms.enrolled",
  version: 1,
  status: "failed",
  attempts: 3,
  maxAttempts: 3,
  availableAt: "2026-10-18T08:00:00.000000Z",
  createdAt: "2026-10-18T08:00:00.000000Z",
  failedAt: "2026-10-18T08:01:00.000000Z",
  lastError: "failed (see the logs for job x)",
  initiatedBy: { id: "7", name: "Staff" },
  ...over,
});

const overview = (canManage: boolean): QueueOverview => ({
  queues: [{ name: "default", pending: 1, scheduled: 0, running: 0, stalled: 0, failed: 1 }],
  workers: { enabled: true, workers: 2, queues: ["default"] },
  canManage,
});

let queries: URLSearchParams[] = [];
let posted: string[] = [];
let manage = true;
let retried = false;
const server = setupServer(
  http.get("*/admin/api/auth/me", () => HttpResponse.json({ user: { id: "1" }, guard: "admin", permissions: [], superAdmin: true })),
  http.get("*/admin/api/auth/csrf", () => HttpResponse.json({ csrfToken: "t", userId: "1" })),
  http.get("*/admin/api/panel", () =>
    HttpResponse.json({
      brand: "T",
      user: { id: "1", name: "A", email: "a@x.y" },
      navigation: [],
      system: { users: false, roles: false, createUsers: false, createRoles: false, audit: false, queue: true },
      settings: [],
    }),
  ),
  http.get("*/admin/api/queue", () => HttpResponse.json(overview(manage))),
  http.get("*/admin/api/queue/jobs", ({ request }) => {
    queries.push(new URL(request.url).searchParams);
    const failed = retried
      ? job("f1", { status: "pending", attempts: 0, failedAt: undefined, lastError: undefined })
      : job("f1");
    return HttpResponse.json({ data: [failed, job("p1", { status: "pending", attempts: 0, failedAt: undefined, lastError: undefined })], meta: {} });
  }),
  http.post("*/admin/api/queue/jobs/:id/:action", ({ params }) => {
    posted.push(`${params.id as string}/${params.action as string}`);
    if (params.id === "p1") {
      return HttpResponse.json(
        { message: "A worker is running this job; wait until it ends.", code: "write_conflict", reason: "running" },
        { status: 409 },
      );
    }
    if (params.action === "retry") retried = true;
    return new HttpResponse(null, { status: 204 });
  }),
);
beforeAll(() => server.listen({ onUnhandledRequest: "error" }));
afterAll(() => server.close());
afterEach(() => {
  server.resetHandlers();
  queries = [];
  posted = [];
  manage = true;
  retried = false;
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

it("shows counts and jobs, and acts after a confirmation", async () => {
  mount("/system/queue");
  const table = await screen.findByRole("table", { name: "Jobs" });
  expect(screen.getByRole("link", { name: "Queue" })).toHaveAttribute("href", "/system/queue");
  expect(screen.getByText("Failed: 1")).toBeInTheDocument();
  expect(within(table).getAllByText("lms.enrolled")).toHaveLength(2);
  expect(within(table).getAllByText("Dispatched by Staff")).toHaveLength(2);
  expect(within(table).getByText("failed (see the logs for job x)")).toBeInTheDocument();

  // Failed: retry or forget; waiting: cancel. Nothing happens before the confirmation.
  const rows = within(table).getAllByRole("row");
  await userEvent.click(within(rows[1]!).getByRole("button", { name: "Retry" }));
  expect(posted).toEqual([]);
  const dialog = await screen.findByRole("dialog", { name: "Run this job again?" });
  await userEvent.click(within(dialog).getByRole("button", { name: "Retry" }));
  expect(await screen.findByText("The job will run again.")).toBeInTheDocument();
  expect(posted).toEqual(["f1/retry"]);
  // The list is read again: the job waits to run and offers a cancel now.
  expect(await within(table).findAllByRole("button", { name: "Cancel job" })).toHaveLength(2);
  expect(within(table).queryByRole("button", { name: "Retry" })).toBeNull();

  // A refusal is explained in the user's language.
  await userEvent.click(within(rows[2]!).getByRole("button", { name: "Cancel job" }));
  await userEvent.click(within(await screen.findByRole("dialog", { name: "Cancel this job?" })).getByRole("button", { name: "Cancel job" }));
  expect(await screen.findByText("A worker is running this job; wait until it ends.")).toBeInTheDocument();
});

it("filters by a count's status and queue", async () => {
  mount("/system/queue");
  await screen.findByRole("table", { name: "Jobs" });
  await userEvent.click(screen.getByText("Failed: 1"));
  await screen.findByRole("table", { name: "Jobs" });
  const last = queries.at(-1)!;
  expect(last.get("queue")).toBe("default");
  expect(last.get("status")).toBe("failed");
});

it("offers no actions without queue.manage", async () => {
  manage = false;
  mount("/system/queue");
  const table = await screen.findByRole("table", { name: "Jobs" });
  expect(within(table).queryByRole("button", { name: "Retry" })).toBeNull();
  expect(within(table).queryByRole("button", { name: "Cancel job" })).toBeNull();
});
