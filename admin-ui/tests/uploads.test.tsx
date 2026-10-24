import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { configure, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { SessionContext } from "@/api/context";
import { createSession } from "@/app/session";
import type { DocNode } from "@/lib/rich-text";
import { RichEditorWidget } from "@/renderers/rich-text";

// The editor is slow to start in jsdom under load.
configure({ asyncUtilTimeout: 4000 });
vi.setConfig({ testTimeout: 20_000 });

const API = "http://localhost/admin/api";
const json = (body: unknown, status = 200) => new Response(JSON.stringify(body), { status, headers: { "Content-Type": "application/json" } });
const idOf = (n: number) => `0199d3a4-1b2c-7d3e-8f40-${String(n).padStart(12, "0")}`;

/** The server: panel limits, CSRF, and uploads answered when `release` is called (in any order). */
function fakeServer({ maxConcurrent = 2, maxBytes = 1000, fail = new Set<string>(), permissions = [] as string[], uploads = true, busy = {} as Record<string, number>, retryAfter = 0, busyLate = false } = {}) {
  const sent: { name: string; key: string }[] = [];
  const pending: { name: string; resolve: () => void }[] = [];
  let inFlight = 0;
  let maxInFlight = 0;
  const failed = new Set<string>();
  vi.stubGlobal("fetch", async (url: string, init: RequestInit) => {
    if (url.endsWith("/panel")) return json({ brand: "x", user: { id: "1", name: "a", email: "a@b.c" }, navigation: [], system: {}, settings: [], pages: [], permissions, ...(uploads ? { uploads: { maxBytes, maxConcurrent } } : {}) });
    if (url.includes("/resources/media/records")) return json({ data: [], meta: { page: 1, perPage: 24, total: 0, lastPage: 1 } });
    if (url.endsWith("/auth/csrf")) return json({ csrfToken: "t", userId: "1" });
    if (url.endsWith("/media")) {
      const file = (init.body as FormData).get("file") as File;
      const key = (init.headers as Record<string, string>)["Idempotency-Key"]!;
      sent.push({ name: file.name, key });
      // The server's upload limit is reached: refused at once (or once released, `busyLate`).
      if (busyLate) await new Promise<void>((resolve) => pending.push({ name: file.name, resolve }));
      if ((busy[file.name] ?? 0) > 0) {
        busy[file.name]! -= 1;
        return new Response(null, { status: 429, headers: retryAfter ? { "Retry-After": String(retryAfter) } : {} });
      }
      inFlight += 1;
      maxInFlight = Math.max(maxInFlight, inFlight);
      await new Promise<void>((resolve) => pending.push({ name: file.name, resolve }));
      inFlight -= 1;
      if (fail.has(file.name) && !failed.has(file.name)) {
        failed.add(file.name);
        return new Response(null, { status: 503 });
      }
      const n = Number(file.name.replace(/\D/g, ""));
      return json({ data: { id: idOf(n), name: file.name, mime: "image/png", size: file.size, inline: true } }, 201);
    }
    throw new Error(`unexpected ${url}`);
  });
  const release = async (name: string) => {
    await waitFor(() => expect(pending.some((p) => p.name === name)).toBe(true));
    const at = pending.findIndex((p) => p.name === name);
    pending.splice(at, 1)[0]!.resolve();
  };
  return { sent, release, maxInFlight: () => maxInFlight, waiting: () => pending.map((p) => p.name) };
}

function renderEditor() {
  let emitted = "";
  let busy = false;
  const client = new QueryClient();
  const session = createSession({ basePath: "/admin", apiPath: API }, client, () => {});
  const { container, unmount } = render(
    <SessionContext.Provider value={session}>
      <QueryClientProvider client={client}>
        <RichEditorWidget id="field-body" label="Body" value="" onChange={(v) => (emitted = v)} invalid={false} nullable onBusy={(b) => (busy = b)} />
      </QueryClientProvider>
    </SessionContext.Provider>,
  );
  const types = () => (emitted ? (JSON.parse(emitted).content as DocNode[]).map((n) => (n.type === "paragraph" ? (n.content?.[0]?.text ?? "") : n.type)) : []);
  const images = () => (emitted ? (JSON.parse(emitted).content as DocNode[]).filter((n) => n.type === "image").map((n) => n.attrs?.media) : []);
  return { container, unmount, images, types, busy: () => busy, session };
}

/** Opens the editor's image picker on its upload tab; returns its file input. */
async function openUpload(tab = true): Promise<HTMLInputElement> {
  const button = screen.getByRole("button", { name: "Insert image" });
  // Enabled once the panel's permissions are in.
  await waitFor(() => expect(button).toBeEnabled());
  await userEvent.click(button);
  const dialog = await screen.findByRole("dialog", { name: "Insert images" });
  if (tab) expect(within(dialog).getByRole("tab", { name: "Upload" })).toHaveAttribute("aria-selected", "true");
  else await userEvent.click(within(dialog).getByRole("tab", { name: "Upload" }));
  return dialog.querySelector("input[type=file]") as HTMLInputElement;
}

const png = (name: string, size = 3) => new File(["x".repeat(size)], name, { type: "image/png" });

afterEach(() => {
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

describe("image uploads in the editor", () => {
  it("send several files at most max_concurrent_uploads at a time, each with its key, inserted in their order", async () => {
    const server = fakeServer({ maxConcurrent: 2 });
    const { images } = renderEditor();
    await userEvent.click(await screen.findByRole("textbox", { name: "Body" }));
    const picker = await openUpload();
    expect(picker.multiple).toBe(true);
    await userEvent.upload(picker, [png("p1.png"), png("p2.png"), png("p3.png")]);
    await waitFor(() => expect(server.waiting()).toEqual(["p1.png", "p2.png"]));
    const dialog = screen.getByRole("dialog", { name: "Insert images" });
    expect(within(dialog).getByRole("list", { name: "Uploads" })).toHaveTextContent("Waiting…");
    // The second finishes first: held back until the first is in.
    await server.release("p2.png");
    await server.release("p3.png");
    await new Promise((r) => setTimeout(r, 20));
    expect(within(dialog).getByText("0 of at most 20 chosen")).toBeInTheDocument();
    await server.release("p1.png");
    await within(dialog).findByText("3 of at most 20 chosen");
    expect(server.maxInFlight()).toBe(2);
    expect(new Set(server.sent.map((s) => s.key)).size).toBe(3);
    await userEvent.click(within(dialog).getByRole("button", { name: "Insert" }));
    await waitFor(() => expect(images()).toEqual([idOf(1), idOf(2), idOf(3)]));
    expect(screen.queryByRole("dialog", { name: "Insert images" })).toBeNull();
  });

  it("show each failure with its own retry (same key) and refuse what is not sent", async () => {
    const server = fakeServer({ maxConcurrent: 4, maxBytes: 10, fail: new Set(["p2.png"]) });
    const { images } = renderEditor();
    await userEvent.click(await screen.findByRole("textbox", { name: "Body" }));
    const picker = await openUpload();
    const pdf = new File(["%PDF"], "doc.pdf", { type: "application/pdf" });
    await userEvent.upload(picker, [png("p1.png"), png("p2.png"), png("big9.png", 11), pdf], { applyAccept: false });
    const dialog = screen.getByRole("dialog", { name: "Insert images" });
    const list = await within(dialog).findByRole("list", { name: "Uploads" });
    expect(within(list).getByText("big9.png is too large.")).toBeInTheDocument();
    expect(within(list).getByText("doc.pdf: this kind of file is not accepted.")).toBeInTheDocument();
    await server.release("p1.png");
    await server.release("p2.png");
    // The first is in; the failed one does not hold it back.
    await within(dialog).findByText("1 of at most 20 chosen");
    const retry = await within(list).findByRole("button", { name: "Retry uploading p2.png" });
    expect(within(list).queryByRole("button", { name: "Retry uploading big9.png" })).toBeNull();
    await userEvent.click(retry);
    await server.release("p2.png");
    await within(dialog).findByText("2 of at most 20 chosen");
    const p2 = server.sent.filter((s) => s.name === "p2.png");
    expect(p2).toHaveLength(2);
    expect(p2[1]!.key).toBe(p2[0]!.key);
    expect(server.sent.map((s) => s.name).sort()).toEqual(["p1.png", "p2.png", "p2.png"]);
    await userEvent.click(within(list).getByRole("button", { name: "Dismiss doc.pdf" }));
    expect(within(list).queryByText(/doc.pdf/)).toBeNull();
    await userEvent.click(within(dialog).getByRole("button", { name: "Insert" }));
    await waitFor(() => expect(images()).toEqual([idOf(1), idOf(2)]));
  });

  it("confirm the picker only once its uploads are in", async () => {
    const server = fakeServer({ maxConcurrent: 2 });
    const { images } = renderEditor();
    await userEvent.click(await screen.findByRole("textbox", { name: "Body" }));
    const picker = await openUpload();
    await userEvent.upload(picker, [png("p1.png"), png("p2.png")]);
    const dialog = screen.getByRole("dialog", { name: "Insert images" });
    await server.release("p1.png");
    await within(dialog).findByText("1 of at most 20 chosen");
    expect(within(dialog).getByRole("button", { name: "Insert" })).toBeDisabled();
    await server.release("p2.png");
    await within(dialog).findByText("2 of at most 20 chosen");
    await userEvent.click(within(dialog).getByRole("button", { name: "Insert" }));
    await waitFor(() => expect(images()).toEqual([idOf(1), idOf(2)]));
  });

  it("put pasted images where they were pasted, even if the cursor moves meanwhile", async () => {
    const server = fakeServer({ maxConcurrent: 2 });
    const { types } = renderEditor();
    const box = await screen.findByRole("textbox", { name: "Body" });
    await userEvent.click(box);
    await userEvent.keyboard("One{Enter}Two");
    await waitFor(() => expect(types()).toEqual(["One", "Two"]));
    // Pasted in "Two", then the cursor goes to "One" before the upload is in.
    fireEvent.paste(box, { clipboardData: { files: [png("p1.png")], types: ["Files"], getData: () => "" } });
    await waitFor(() => expect(server.waiting()).toEqual(["p1.png"]));
    await userEvent.click(box.querySelector("p")!);
    await userEvent.keyboard("0");
    await waitFor(() => expect(types()[0]).toBe("One0"));
    await server.release("p1.png");
    await waitFor(() => expect(types()).toEqual(["One0", "Two", "image", ""]));
  });

  it("start each pasted batch where it is pasted, whatever failures stay listed", async () => {
    const server = fakeServer({ maxConcurrent: 2 });
    const { types } = renderEditor();
    const box = await screen.findByRole("textbox", { name: "Body" });
    await userEvent.click(box);
    await userEvent.keyboard("One{Enter}Two");
    await waitFor(() => expect(types()).toEqual(["One", "Two"]));
    // A refused file pasted in "Two" stays listed...
    const pdf = new File(["%PDF"], "doc.pdf", { type: "application/pdf" });
    fireEvent.paste(box, { clipboardData: { files: [pdf], types: ["Files"], getData: () => "" } });
    await screen.findByText("doc.pdf: only images can be inserted.");
    // ...then an image pasted in "One" goes after "One".
    await userEvent.click(box.querySelector("p")!);
    fireEvent.paste(box, { clipboardData: { files: [png("p1.png")], types: ["Files"], getData: () => "" } });
    await server.release("p1.png");
    await waitFor(() => expect(types()).toEqual(["One", "image", "Two"]));
  });

  it("never send queued files in another session", async () => {
    const server = fakeServer({ maxConcurrent: 1 });
    const { session } = renderEditor();
    await userEvent.click(await screen.findByRole("textbox", { name: "Body" }));
    const picker = await openUpload();
    await userEvent.upload(picker, [png("p1.png"), png("p2.png")]);
    await waitFor(() => expect(server.waiting()).toEqual(["p1.png"]));
    // Signed out, or another account, while the first is on its way.
    session.api.resetCsrf();
    await server.release("p1.png");
    const list = screen.getByRole("list", { name: "Uploads" });
    await within(list).findByText("Your session changed before this was sent; nothing was saved.");
    await new Promise((r) => setTimeout(r, 50));
    expect(server.sent.map((s) => s.name)).toEqual(["p1.png"]);
    expect(within(list).queryByRole("button", { name: "Retry uploading p2.png" })).toBeNull();
  });

  it("keep the picker's uploads while switching tabs", async () => {
    const server = fakeServer({ maxConcurrent: 2, permissions: ["media.view_any", "media.view"] });
    renderEditor();
    await userEvent.click(await screen.findByRole("textbox", { name: "Body" }));
    const picker = await openUpload(false);
    await userEvent.upload(picker, [png("p1.png")]);
    await waitFor(() => expect(server.waiting()).toEqual(["p1.png"]));
    const dialog = screen.getByRole("dialog", { name: "Insert images" });
    await userEvent.click(within(dialog).getByRole("tab", { name: "Library" }));
    await server.release("p1.png");
    await within(dialog).findByText("1 of at most 20 chosen");
    await userEvent.click(within(dialog).getByRole("tab", { name: "Upload" }));
    expect(server.sent).toHaveLength(1);
  });

  it("upload dropped and pasted files; pasted external images stay dropped", async () => {
    const server = fakeServer({ maxConcurrent: 4 });
    const { images, busy } = renderEditor();
    const box = await screen.findByRole("textbox", { name: "Body" });
    await userEvent.click(box);
    await new Promise((r) => setTimeout(r, 50));
    fireEvent.drop(box, { dataTransfer: { files: [png("p1.png")], types: ["Files"], getData: () => "" }, clientX: 0, clientY: 0 });
    // The form waits for the upload.
    await waitFor(() => expect(busy()).toBe(true));
    expect(screen.getByRole("list", { name: "Uploads" })).toHaveTextContent("p1.png");
    await server.release("p1.png");
    await waitFor(() => expect(images()).toEqual([idOf(1)]));
    const html = '<img src="https://evil.test/x.png">';
    fireEvent.paste(box, { clipboardData: { files: [png("p2.png")], types: ["Files", "text/html"], getData: (t: string) => (t === "text/html" ? html : "") } });
    await server.release("p2.png");
    // Right after the block holding the cursor (above the first image).
    await waitFor(() => expect(images()).toEqual([idOf(2), idOf(1)]));
    // Only an external image pasted (no file): nothing is uploaded or kept.
    fireEvent.paste(box, { clipboardData: { files: [], types: ["text/html"], getData: (t: string) => (t === "text/html" ? html : "") } });
    await new Promise((r) => setTimeout(r, 20));
    expect(images()).toEqual([idOf(2), idOf(1)]);
    expect(box.querySelector('img[src^="https://evil"]')).toBeNull();
    expect(server.sent.map((s) => s.name)).toEqual(["p1.png", "p2.png"]);
    await waitFor(() => expect(busy()).toBe(false));
  });

  it("ignore dropped and pasted files without the right to upload, and say so", async () => {
    const server = fakeServer({ uploads: false });
    const { types } = renderEditor();
    const box = await screen.findByRole("textbox", { name: "Body" });
    await userEvent.click(box);
    // The panel is in: no upload limits, so no upload.
    await waitFor(() => expect(screen.getByRole("button", { name: "Insert image" })).toBeDisabled());
    await new Promise((r) => setTimeout(r, 50));
    fireEvent.paste(box, { clipboardData: { files: [png("p1.png")], types: ["Files"], getData: () => "" } });
    const status = await screen.findByText("You may not upload files: the dropped or pasted files were not added.");
    expect(status).toHaveAttribute("role", "status");
    // Typing clears the message; a drop shows it again.
    await userEvent.keyboard("Hi");
    await waitFor(() => expect(status).toBeEmptyDOMElement());
    fireEvent.drop(box, { dataTransfer: { files: [png("p2.png")], types: ["Files"], getData: () => "" }, clientX: 0, clientY: 0 });
    await screen.findByText("You may not upload files: the dropped or pasted files were not added.");
    await new Promise((r) => setTimeout(r, 50));
    expect(server.sent).toEqual([]);
    expect(screen.queryByRole("list", { name: "Uploads" })).toBeNull();
    expect(types()).toEqual(["Hi"]);
  });

  it("never send more than two at once, whatever the server allows (its limit is shared)", async () => {
    const server = fakeServer({ maxConcurrent: 8 });
    renderEditor();
    await userEvent.click(await screen.findByRole("textbox", { name: "Body" }));
    const picker = await openUpload();
    await userEvent.upload(picker, [png("p1.png"), png("p2.png"), png("p3.png")]);
    await waitFor(() => expect(server.waiting()).toEqual(["p1.png", "p2.png"]));
    await server.release("p1.png");
    await waitFor(() => expect(server.waiting()).toEqual(["p2.png", "p3.png"]));
    await server.release("p2.png");
    await server.release("p3.png");
    await within(screen.getByRole("dialog", { name: "Insert images" })).findByText("3 of at most 20 chosen");
    expect(server.maxInFlight()).toBe(2);
  });

  it("send a file refused with 429 again by itself after Retry-After (5 s without), the same key, at most three times", async () => {
    const server = fakeServer({ busy: { "p1.png": 4 } });
    renderEditor();
    await userEvent.click(await screen.findByRole("textbox", { name: "Body" }));
    const picker = await openUpload();
    vi.useFakeTimers({ shouldAdvanceTime: true });
    await userEvent.upload(picker, [png("p1.png")]);
    const list = await screen.findByRole("list", { name: "Uploads" });
    await within(list).findByText("The server is busy: trying again in 5 s…");
    expect(server.sent).toHaveLength(1);
    // Not before its time.
    await vi.advanceTimersByTimeAsync(4000);
    expect(server.sent).toHaveLength(1);
    for (const sends of [2, 3, 4]) {
      await vi.advanceTimersByTimeAsync(1000);
      await waitFor(() => expect(server.sent).toHaveLength(sends));
      if (sends < 4) await within(list).findByText("The server is busy: trying again in 5 s…");
      else await vi.advanceTimersByTimeAsync(5000);
    }
    // Three times again, then a failure with its retry.
    const retry = await within(list).findByRole("button", { name: "Retry uploading p1.png" });
    expect(within(list).getByRole("alert")).toHaveTextContent("Too many attempts");
    expect(server.sent).toHaveLength(4);
    fireEvent.click(retry);
    await server.release("p1.png");
    await within(screen.getByRole("dialog", { name: "Insert images" })).findByText("1 of at most 20 chosen");
    expect(new Set(server.sent.map((s) => s.key)).size).toBe(1);
    expect(server.sent).toHaveLength(5);
  });

  it("wait as long as Retry-After says, and never send a waiting file in another session", async () => {
    const server = fakeServer({ busy: { "p1.png": 1, "p2.png": 1 }, retryAfter: 2 });
    const { session } = renderEditor();
    await userEvent.click(await screen.findByRole("textbox", { name: "Body" }));
    const picker = await openUpload();
    vi.useFakeTimers({ shouldAdvanceTime: true });
    await userEvent.upload(picker, [png("p1.png")]);
    const list = await screen.findByRole("list", { name: "Uploads" });
    await within(list).findByText("The server is busy: trying again in 2 s…");
    await vi.advanceTimersByTimeAsync(2000);
    await server.release("p1.png");
    const dialog = screen.getByRole("dialog", { name: "Insert images" });
    await within(dialog).findByText("1 of at most 20 chosen");
    expect(server.sent.map((s) => s.name)).toEqual(["p1.png", "p1.png"]);
    // Waiting for its retry when the session changes: never sent again.
    await userEvent.upload(dialog.querySelector("input[type=file]") as HTMLInputElement, [png("p2.png")]);
    // A new list (the first emptied once its file was in).
    const again = await screen.findByRole("list", { name: "Uploads" });
    await within(again).findByText("The server is busy: trying again in 2 s…");
    session.api.resetCsrf();
    await within(again).findByText("Your session changed before this was sent; nothing was saved.");
    await vi.advanceTimersByTimeAsync(5000);
    expect(server.sent.map((s) => s.name)).toEqual(["p1.png", "p1.png", "p2.png"]);
    expect(within(again).queryByRole("button", { name: "Retry uploading p2.png" })).toBeNull();
  });

  it("never wait to send again a file refused with 429 once the editor is gone", async () => {
    const server = fakeServer({ busy: { "p1.png": 1 }, busyLate: true });
    const { unmount } = renderEditor();
    const box = await screen.findByRole("textbox", { name: "Body" });
    await userEvent.click(box);
    await new Promise((r) => setTimeout(r, 50));
    fireEvent.drop(box, { dataTransfer: { files: [png("p1.png")], types: ["Files"], getData: () => "" }, clientX: 0, clientY: 0 });
    await waitFor(() => expect(server.waiting()).toEqual(["p1.png"]));
    unmount();
    const timeouts = vi.spyOn(globalThis, "setTimeout");
    await server.release("p1.png");
    await new Promise((r) => setTimeout(r, 50));
    // No retry scheduled (5 s, the default without Retry-After).
    expect(timeouts.mock.calls.filter(([, ms]) => ms === 5000)).toEqual([]);
    expect(server.sent).toHaveLength(1);
    timeouts.mockRestore();
  });
});
