// Queue history, job details, auto refresh, and the Schedules page (0.8.0).
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { RouterProvider, createMemoryHistory } from "@tanstack/react-router";
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { http, HttpResponse } from "msw";
import { setupServer } from "msw/node";
import { SessionContext } from "@/api/context";
import { createAppRouter } from "@/app/router";
import { createSession } from "@/app/session";
import { formatDuration, sinceOf } from "@/components/refresh";
import { REFRESH_MS, interval } from "@/lib/refresh";

const run = (id: string, over: object = {}) => ({
  id, jobId: `job-${id}`, queue: "default", kind: "deploy.run", description: `deploy demo/web #${id}`, status: "done",
  attempts: 1, createdAt: "2026-10-20T08:00:00.000000Z", startedAt: "2026-10-20T08:00:01.000000Z",
  finishedAt: "2026-10-20T08:00:04.000000Z", durationMs: 3200, ...over,
});

const schedule = (name: string, over: object = {}) => ({
  name, rule: "every 3600s", action: { kind: "closure" }, withoutOverlapping: false,
  nextAt: "2026-10-20T10:00:00.000000Z", ...over,
});

let manage = true;
let schedulerOn = true;
let historyQueries: URLSearchParams[] = [];
let runs: string[] = [];
let jobsPages = 0;
const server = setupServer(
  http.get("*/admin/api/auth/me", () => HttpResponse.json({ user: { id: "1" }, guard: "admin", permissions: [], superAdmin: true })),
  http.get("*/admin/api/auth/csrf", () => HttpResponse.json({ csrfToken: "t", userId: "1" })),
  http.get("*/admin/api/panel", () =>
    HttpResponse.json({
      brand: "T", user: { id: "1", name: "A", email: "a@x.y" }, navigation: [], settings: [],
      system: { users: false, roles: false, createUsers: false, createRoles: false, audit: false, queue: true, schedules: true },
    })),
  http.get("*/admin/api/queue", () => HttpResponse.json({
    queues: [{ name: "default", pending: 0, scheduled: 2, running: 1, stalled: 0, failed: 0 }],
    workers: { enabled: true, workers: 2, queues: ["default"] }, canManage: true, lastDay: { done: 12, failed: 1 },
  })),
  http.get("*/admin/api/queue/jobs", ({ request }) => {
    jobsPages++;
    const cursor = new URL(request.url).searchParams.get("cursor");
    return HttpResponse.json({
      data: [{
        id: cursor ? "j2" : "j1", queue: "default", kind: "backup.run", version: 1, status: "running", attempts: 1, maxAttempts: 1,
        availableAt: "2026-10-20T08:00:00.000000Z", createdAt: "2026-10-20T08:00:00.000000Z", startedAt: "2026-10-20T08:00:00.000000Z",
        description: "backup db",
      }],
      meta: cursor ? {} : { nextCursor: "c1" },
    });
  }),
  http.get("*/admin/api/queue/history", ({ request }) => {
    historyQueries.push(new URL(request.url).searchParams);
    return HttpResponse.json({ data: [run("1"), run("2", { status: "failed", lastError: "smtp_down: the mail server is down" })], meta: {} });
  }),
  http.get("*/admin/api/queue/jobs/:id", () => HttpResponse.json({ runs: [run("1")] })),
  http.get("*/admin/api/schedules", () => HttpResponse.json({
    data: [
      schedule("database-backups", { last: { id: "r1", trigger: "schedule", startedAt: "2026-10-20T03:00:00.000000Z", status: "succeeded", durationMs: 65000, summary: "3 databases backed up" } }),
      schedule("disk-check", { last: { id: "r2", trigger: "schedule", startedAt: "2026-10-20T03:00:00.000000Z", status: "unknown", error: "interrupted by shutdown" } }),
      schedule("reap", { action: { kind: "job", jobKind: "deploy.reap" }, withoutOverlapping: true }),
    ],
    scheduler: { enabled: schedulerOn, graceSecs: 30 }, workers: true, canManage: manage,
  })),
  http.get("*/admin/api/schedules/:name/runs", () => HttpResponse.json({
    data: [{ id: "r1", trigger: "manual", startedAt: "2026-10-20T03:00:00.000000Z", status: "failed", error: "smtp_down: down", jobId: "j9" }],
    meta: {},
  })),
  http.post("*/admin/api/schedules/:name/run", ({ params }) => {
    runs.push(params.name as string);
    return HttpResponse.json({ run: { id: "r9", trigger: "manual", startedAt: "2026-10-20T09:00:00.000000Z", status: "dispatched", jobId: "j9" } }, { status: 202 });
  }),
);
beforeAll(() => server.listen({ onUnhandledRequest: "error" }));
afterAll(() => server.close());
afterEach(() => {
  server.resetHandlers();
  manage = true;
  schedulerOn = true;
  historyQueries = [];
  runs = [];
  jobsPages = 0;
  try {
    window.localStorage.clear();
  } catch {
    // ignore
  }
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

describe("helpers", () => {
  it("polls only when on, visible and not held", () => {
    expect(interval(true, true, false)).toBe(REFRESH_MS);
    expect(interval(false, true, false)).toBe(false);
    expect(interval(true, false, false)).toBe(false);
    expect(interval(true, true, true)).toBe(false);
  });

  it("formats durations and ranges", () => {
    expect(formatDuration(850)).toBe("850 ms");
    expect(formatDuration(3200)).toBe("3.2 s");
    expect(formatDuration(65_000)).toBe("1 min 5 s");
    expect(formatDuration(undefined)).toBe("—");
    expect(sinceOf("1h", new Date("2026-10-20T10:00:00Z"))).toBe("2026-10-20T09:00:00.000Z");
    expect(sinceOf("")).toBeUndefined();
  });
});

describe("Queue", () => {
  it("says Delayed, shows running jobs' start, and the last day's counts", async () => {
    mount("/system/queue");
    const table = await screen.findByRole("table", { name: "Jobs" });
    expect(screen.getByText("Delayed: 2")).toBeInTheDocument();
    expect(screen.getByText(/Recurring tasks are under System → Schedules/)).toBeInTheDocument();
    expect(screen.getByText("Last 24 hours: 12 done, 1 failed.")).toBeInTheDocument();
    expect(within(table).getByText("backup db")).toBeInTheDocument();
    expect(within(table).getByText(/^Started /)).toBeInTheDocument();
  });

  it("lists the history with a time range, and opens a job's details from the keyboard", async () => {
    mount("/system/queue");
    await screen.findByRole("table", { name: "Jobs" });
    await userEvent.click(screen.getByRole("tab", { name: "History" }));
    const table = await screen.findByRole("table", { name: "History" });
    expect(within(table).getByText("deploy demo/web #1")).toBeInTheDocument();
    expect(within(table).getAllByText("3.2 s")).toHaveLength(2);
    expect(within(table).getByText("smtp_down: the mail server is down")).toBeInTheDocument();
    await userEvent.selectOptions(screen.getByLabelText("Time"), "24h");
    await userEvent.click(screen.getByRole("button", { name: "Filter" }));
    await vi.waitFor(() => expect(historyQueries.at(-1)?.get("since")).toBeTruthy());
    // The kind is a button: Enter opens the details (the filtered list is a new table).
    const filtered = await screen.findByRole("table", { name: "History" });
    const open = (await within(filtered).findAllByRole("button", { name: "deploy.run" }))[0]!;
    open.focus();
    await userEvent.keyboard("{Enter}");
    const dialog = await screen.findByRole("dialog", { name: "Job details" });
    expect(within(dialog).getByText("Done")).toBeInTheDocument();
  });

  it("pauses auto refresh while older rows are loaded, until back to latest", async () => {
    mount("/system/queue");
    await screen.findByRole("table", { name: "Jobs" });
    expect(screen.getByRole("switch", { name: "Auto refresh" })).toHaveAttribute("aria-checked", "true");
    await userEvent.click(screen.getByRole("button", { name: "Load more" }));
    expect(await screen.findByText(/Auto refresh is paused/)).toBeInTheDocument();
    const before = jobsPages;
    await userEvent.click(screen.getByRole("button", { name: "Back to latest" }));
    await vi.waitFor(() => expect(jobsPages).toBeGreaterThan(before));
    await vi.waitFor(() => expect(screen.queryByText(/Auto refresh is paused/)).toBeNull());
    // The choice is kept for this browser.
    await userEvent.click(screen.getByRole("switch", { name: "Auto refresh" }));
    expect(window.localStorage.getItem("rustcms.autoRefresh")).toBe("off");
  });
});

describe("Schedules", () => {
  it("lists schedules with their next and last runs, and runs one now after a confirmation", async () => {
    mount("/system/schedules");
    const table = await screen.findByRole("table", { name: "Schedules" });
    expect(screen.getByText(/Jobs waiting to run later are under Queue → Delayed/)).toBeInTheDocument();
    expect(within(table).getByText("3 databases backed up")).toBeInTheDocument();
    expect(within(table).getByText("Completion unknown")).toBeInTheDocument();
    expect(within(table).getByText("deploy.reap")).toBeInTheDocument();
    expect(within(table).getByText("Waits for the previous job")).toBeInTheDocument();
    const rows = within(table).getAllByRole("row");
    await userEvent.click(within(rows[1]!).getByRole("button", { name: "Run now" }));
    expect(runs).toEqual([]);
    const dialog = await screen.findByRole("dialog", { name: "Run database-backups now?" });
    await userEvent.click(within(dialog).getByRole("button", { name: "Run now" }));
    expect(await screen.findByText("database-backups was started; its run appears below.")).toBeInTheDocument();
    expect(runs).toEqual(["database-backups"]);
  });

  it("opens the runs of a schedule", async () => {
    mount("/system/schedules");
    const table = await screen.findByRole("table", { name: "Schedules" });
    await userEvent.click(within(table).getByRole("button", { name: "disk-check" }));
    const dialog = await screen.findByRole("dialog", { name: "Runs of disk-check" });
    expect(await within(dialog).findByText("smtp_down: down")).toBeInTheDocument();
    expect(within(dialog).getByText("Run now")).toBeInTheDocument();
    expect(within(dialog).getByText("Job j9")).toBeInTheDocument();
  });

  it("offers no Run now without schedules.manage, and says when the scheduler is off", async () => {
    manage = false;
    schedulerOn = false;
    mount("/system/schedules");
    const table = await screen.findByRole("table", { name: "Schedules" });
    expect(within(table).queryByRole("button", { name: "Run now" })).toBeNull();
    expect(screen.getByText(/The scheduler is off on this instance/)).toBeInTheDocument();
    expect(screen.getByRole("link", { name: "Schedules" })).toHaveAttribute("href", "/system/schedules");
  });
});
