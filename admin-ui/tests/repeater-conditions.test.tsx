// Conditions on and in repeaters (ADR 0044, part A): rows read the record's
// form (`when_root`); a relationship repeater the user's changes hide says
// that saving removes its rows, and its changes are not sent.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, within } from "@testing-library/react";
import { http, HttpResponse } from "msw";
import { setupServer } from "msw/node";
import { useForm } from "react-hook-form";
import { SessionContext } from "@/api/context";
import type { ComponentSchema, InfolistSchema, InputSchema } from "@/api/types";
import { createSession } from "@/app/session";
import { Components } from "@/renderers/form";
import { InfolistRenderer } from "@/renderers/infolist";
import { type FormValues, defaultValues, hiddenNames, inputsOf, payloadOf } from "@/schema/form";
import { rowProblems } from "@/schema/repeater";
import edit from "../../fixtures/contract/repeater-conditions.schema.edit.json";

const server = setupServer(
  http.get("*/admin/api/resources/quizzes/records/:key/relations/options", () =>
    HttpResponse.json({
      data: [
        { key: "1", abilities: { update: true, delete: true }, data: { id: 1, content: "a", correct: true, hint: null, position: 1 } },
        { key: "2", abilities: { update: true, delete: true }, data: { id: 2, content: "b", correct: false, hint: null, position: 2 } },
      ],
      meta: { total: 2, page: 1, perPage: 100, lastPage: 1 },
    })),
);
beforeAll(() => server.listen({ onUnhandledRequest: "error" }));
afterEach(() => server.resetHandlers());
afterAll(() => server.close());

const text = (name: string, extra: Partial<InputSchema> = {}) =>
  ({ type: "text_input", name, label: name[0]!.toUpperCase() + name.slice(1), valueType: "string", nullable: true, disabled: false, rules: [], columnSpan: 1, ...extra }) as InputSchema;
const explained = { op: "is_true", name: "explain", scope: "root" } as const;

// The server's edit form (fixtures/contract/repeater-conditions.*, from tests/repeater_conditions.rs).
const components = edit.form.components as unknown as ComponentSchema[];
const inputs = inputsOf(components);

let form: ReturnType<typeof useForm<FormValues>> | undefined;
function Harness({ record }: { record?: Record<string, unknown> }) {
  form = useForm<FormValues>({ defaultValues: defaultValues(inputs, record) });
  return <Components components={components} ctx={{ slug: "quizzes", record: record ? "7" : undefined, control: form.control, errors: form.formState.errors }} />;
}

function mount(record?: Record<string, unknown>) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const session = createSession({ basePath: "", apiPath: "http://localhost/admin/api" }, client, () => {});
  render(<SessionContext.Provider value={session}><QueryClientProvider client={client}><Harness record={record} /></QueryClientProvider></SessionContext.Provider>);
}

const stored = { title: "Q", kind: "choice", explain: false, links: [{ label: "Docs", note: "n" }] };

it("shows a row's input as the form's values read (when_root), as they change", async () => {
  mount(stored);
  await screen.findByDisplayValue("a");
  // Not explaining: neither the link's note nor a correct option's hint.
  expect(screen.queryByLabelText(/^Note/)).toBeNull();
  expect(screen.queryByLabelText(/^Hint/)).toBeNull();
  fireEvent.click(screen.getByRole("switch", { name: /^Explain/ }));
  expect(await screen.findByLabelText(/^Note/)).toBeInTheDocument();
  // Only the correct option ("a") explains.
  expect(screen.getAllByLabelText(/^Hint/)).toHaveLength(1);
});

it("warns that saving removes the rows of a repeater the change hides, and sends none of its changes", async () => {
  mount(stored);
  await screen.findByDisplayValue("a");
  fireEvent.change(screen.getAllByLabelText(/^Content/)[1]!, { target: { value: "bb" } });
  fireEvent.change(screen.getByLabelText(/^Kind/), { target: { value: JSON.stringify("blank") } });
  expect(await screen.findByRole("status")).toHaveTextContent("Saving removes the 2 rows of Options you can see.");
  expect(screen.queryByDisplayValue("a")).toBeNull();
  // The form leaves out what its conditions hide, as when it saves.
  const values = form!.getValues();
  expect(payloadOf(inputs, values, undefined, hiddenNames(components, values)).options).toBeUndefined();
  // Shown again: the draft is back, and no warning.
  fireEvent.change(screen.getByLabelText(/^Kind/), { target: { value: JSON.stringify("choice") } });
  expect(await screen.findByDisplayValue("bb")).toBeInTheDocument();
  expect(screen.queryByRole("status")).toBeNull();
});

it("says nothing when the repeater was hidden when the form opened", async () => {
  mount({ ...stored, kind: "blank" });
  await screen.findByLabelText(/^Kind/);
  expect(screen.queryByRole("status")).toBeNull();
});

it("checks inputs under a condition on the form only where the form is known", () => {
  const links = inputs.find((i) => i.name === "links")!.repeater!;
  const rows = [{ label: "", note: "" }];
  // Before a save the form is not known here: only what the row decides is checked.
  expect(Object.keys(rowProblems(links, rows))).toEqual(["0.label"]);
  const required = { ...links, components: [text("label", { rules: [{ rule: "required" }] }), text("note", { shownWhen: explained, rules: [{ rule: "required" }] })] };
  expect(Object.keys(rowProblems(required, rows))).toEqual(["0.label"]);
  const root = (explain: boolean) => ({ root: { values: { explain }, kinds: new Map([["explain", "bool"]]) } });
  expect(Object.keys(rowProblems(required, rows, "", root(true))).sort()).toEqual(["0.label", "0.note"]);
  expect(Object.keys(rowProblems(required, rows, "", root(false)))).toEqual(["0.label"]);
  expect(within(document.body).queryByRole("alert")).toBeNull();
});

it("shows a relation's rows on the view page, in the form's order, from the relation's list", async () => {
  let asked: URLSearchParams | undefined;
  server.use(
    http.get("*/admin/api/resources/quizzes/records/:key/relations/options", ({ request }) => {
      asked = new URL(request.url).searchParams;
      return HttpResponse.json({
        data: [
          { key: "1", abilities: {}, data: { content: "first", position: 1 } },
          { key: "2", abilities: {}, data: { content: "second", position: 2 } },
        ],
        meta: { total: 3, page: 1, perPage: 2, lastPage: 2 },
      });
    }),
  );
  const infolist = {
    columns: 1,
    components: [{
      type: "repeatable_entry", name: "options", label: "Options", valueType: "rows", columnSpan: 1,
      rows: { resource: "quiz_options", relation: "options", orderBy: "position", limit: 2 },
      components: [{ type: "text_entry", name: "content", label: "Content", valueType: "string", columnSpan: 1 }],
    }],
  } as unknown as InfolistSchema;
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const session = createSession({ basePath: "", apiPath: "http://localhost/admin/api" }, client, () => {});
  render(
    <SessionContext.Provider value={session}><QueryClientProvider client={client}>
      <InfolistRenderer infolist={infolist} slug="quizzes" recordKey="7" data={{ title: "Q" }} />
    </QueryClientProvider></SessionContext.Provider>,
  );
  await screen.findByText("first");
  expect(screen.getAllByRole("listitem").map((li) => li.textContent)).toEqual(["Contentfirst", "Contentsecond"]);
  // The server sorts: the first rows by position, whatever the limit.
  expect([asked?.get("sort"), asked?.get("per_page")]).toEqual(["position", "2"]);
  // More rows than shown: the relation's section has them all.
  expect(screen.getByRole("link", { name: "All 3 rows" })).toHaveAttribute("href", "#relation-options");
});

it("warns too when a section around the repeater hides it", async () => {
  // The options' condition moved to a section around them.
  const options = components.find((c) => "name" in c && c.name === "options") as InputSchema;
  const wrapped = components.map((c) =>
    c === options
      ? ({ type: "section", label: "Answers", columns: 1, shownWhen: options.shownWhen, components: [{ ...options, shownWhen: undefined }] } as unknown as ComponentSchema)
      : c);
  function Wrapped() {
    const form = useForm<FormValues>({ defaultValues: defaultValues(inputsOf(wrapped), stored) });
    return <Components components={wrapped} ctx={{ slug: "quizzes", record: "7", control: form.control, errors: form.formState.errors }} />;
  }
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const session = createSession({ basePath: "", apiPath: "http://localhost/admin/api" }, client, () => {});
  render(<SessionContext.Provider value={session}><QueryClientProvider client={client}><Wrapped /></QueryClientProvider></SessionContext.Provider>);
  await screen.findByDisplayValue("a");
  fireEvent.change(screen.getByLabelText(/^Kind/), { target: { value: JSON.stringify("fill_blank") } });
  expect(await screen.findByRole("status")).toHaveTextContent("Saving removes the 2 rows of Options you can see.");
});

it("shows a relation row's inputs under conditions on the record (when_root) on the view page", async () => {
  server.use(
    http.get("*/admin/api/resources/quizzes/records/:key/relations/options", () =>
      HttpResponse.json({
        data: [{ key: "1", abilities: {}, data: { content: "a", correct: true, hint: "why" } }],
        meta: { total: 1, page: 1, perPage: 100, lastPage: 1 },
      })),
  );
  const hint = { op: "all", conditions: [{ op: "is_true", name: "correct" }, { op: "is_true", name: "explain", scope: "root" }] };
  const infolist = {
    columns: 1,
    components: [{
      type: "repeatable_entry", name: "options", label: "Options", valueType: "rows", columnSpan: 1,
      rows: { resource: "quiz_options", relation: "options", limit: 100 },
      components: [
        { type: "boolean_entry", name: "correct", label: "Correct", valueType: "bool", columnSpan: 1 },
        { type: "text_entry", name: "hint", label: "Hint", valueType: "string", columnSpan: 1, shownWhen: hint },
      ],
    }],
  } as unknown as InfolistSchema;
  const view = (explain: boolean) => {
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    const session = createSession({ basePath: "", apiPath: "http://localhost/admin/api" }, client, () => {});
    return render(
      <SessionContext.Provider value={session}><QueryClientProvider client={client}>
        <InfolistRenderer infolist={infolist} slug="quizzes" recordKey="7" data={{ explain }} />
      </QueryClientProvider></SessionContext.Provider>,
    );
  };
  const explained = view(true);
  expect(await screen.findByText("why")).toBeInTheDocument();
  explained.unmount();
  view(false);
  await screen.findByText("Correct");
  expect(screen.queryByText("why")).toBeNull();
});
