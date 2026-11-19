// Nested relationship repeaters (ADR 0044, part C): a question's blanks, each
// with its choices. A stored blank reads its choices through its own relation;
// a save sends the choices' change inside the blank's; errors are named by
// every level.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { RouterProvider, createMemoryHistory, createRootRoute, createRouter } from "@tanstack/react-router";
import { fireEvent, render, screen } from "@testing-library/react";
import { http, HttpResponse } from "msw";
import { setupServer } from "msw/node";
import { useForm } from "react-hook-form";
import { SessionContext } from "@/api/context";
import type { ComponentSchema, InfolistSchema, InputSchema } from "@/api/types";
import { createSession } from "@/app/session";
import { Components } from "@/renderers/form";
import { InfolistRenderer } from "@/renderers/infolist";
import { type FormValues, defaultValues, fromFormValue, inputsOf, serverErrors } from "@/schema/form";
import { applyPatches, reactionState } from "@/schema/reactions";
import { type RelationState, changeOf, relationRowProblems } from "@/schema/relation-rows";
import edit from "../../fixtures/contract/repeater-nested.schema.edit.json";

let choicesAsked: string[] = [];
const server = setupServer(
  http.get("*/admin/api/resources/questions/records/:key/relations/options", () =>
    HttpResponse.json({ data: [], meta: { total: 0, page: 1, perPage: 100, lastPage: 1 } })),
  http.get("*/admin/api/resources/questions/records/:key/relations/blanks", () =>
    HttpResponse.json({
      data: [{ key: "1", abilities: { update: true, delete: true }, data: { id: 1, label: "First", mode: "select", position: 1, hints: null } }],
      meta: { total: 1, page: 1, perPage: 100, lastPage: 1 },
    })),
  http.get("*/admin/api/resources/question_blanks/records/:key/relations/choices", ({ params }) => {
    choicesAsked.push(String(params.key));
    return HttpResponse.json({
      data: [{ key: "11", abilities: { update: true, delete: true }, data: { id: 11, content: "yes", note: "because", position: 1 } }],
      meta: { total: 1, page: 1, perPage: 100, lastPage: 1 },
    });
  }),
);
beforeAll(() => server.listen({ onUnhandledRequest: "error" }));
afterEach(() => {
  server.resetHandlers();
  choicesAsked = [];
});
afterAll(() => server.close());

// The server's edit form (fixtures/contract/repeater-nested.*, from tests/repeater_nested.rs).
const components = edit.form.components as unknown as ComponentSchema[];
const inputs = inputsOf(components);
const blanks = inputs.find((i) => i.name === "blanks")!;
const choices = inputsOf(blanks.repeater!.components).find((i) => i.name === "choices")!;

let form: ReturnType<typeof useForm<FormValues>> | undefined;
function Harness({ record }: { record: Record<string, unknown> }) {
  form = useForm<FormValues>({ defaultValues: defaultValues(inputs, record) });
  return <Components components={components} ctx={{ slug: "questions", record: "7", control: form.control, errors: form.formState.errors }} />;
}

function mount(record: Record<string, unknown>) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const session = createSession({ basePath: "", apiPath: "http://localhost/admin/api" }, client, () => {});
  render(<SessionContext.Provider value={session}><QueryClientProvider client={client}><Harness record={record} /></QueryClientProvider></SessionContext.Provider>);
}

const state = (value: RelationState) => JSON.stringify(value);

it("sends a row's rows inside its change, even when the row itself is unchanged", () => {
  const nested = state({ rows: [{ key: "11", values: { content: "yes!", note: "" }, changed: ["content"] }], deleted: ["12"] });
  const created = state({ rows: [{ ref: "n9", values: { content: "new", note: "" }, changed: [] }], deleted: [] });
  const value = state({
    rows: [
      { key: "1", values: { label: "First", mode: JSON.stringify("select"), hints: "", choices: nested }, changed: ["choices"] },
      { ref: "n1", values: { label: "Second", mode: JSON.stringify("text"), hints: "", choices: created }, changed: [] },
    ],
    deleted: [],
  });
  expect(changeOf(blanks, value)).toEqual({
    update: [{ key: "1", data: { choices: { update: [{ key: "11", data: { content: "yes!" } }], delete: ["12"] } } }],
    create: [{ ref: "n1", data: { label: "Second", mode: "text", choices: { create: [{ ref: "n9", data: { content: "new" } }] } } }],
  });
  // Unchanged rows send nothing below.
  expect(fromFormValue(choices, "")).toBeUndefined();
});

it("names the server's problems by every level", () => {
  const { byInput } = serverErrors({ "blanks.n1.choices.n2.content": ["is too long"], "blanks.1.choices": ["is invalid"] }, new Set(["blanks"]));
  expect(byInput.blanks!.types).toEqual({ "n1.choices.n2.content": "is too long", "1.choices": "is invalid" });
});

it("checks the rows of rows where the panel can", () => {
  const nested = state({ rows: [{ ref: "n2", values: { content: "", note: "" }, changed: [] }], deleted: [] });
  const rows = [{ ref: "n1", values: { label: "x", mode: JSON.stringify("select"), hints: "", choices: nested }, changed: [] }];
  const required = {
    ...blanks.repeater!,
    components: blanks.repeater!.components.map((c) =>
      "name" in c && c.name === "choices"
        ? { ...c, repeater: { ...(c as InputSchema).repeater!, components: [{ ...(c as InputSchema).repeater!.components[0], rules: [{ rule: "required" }] }] } }
        : c),
  } as typeof blanks.repeater & object;
  expect(Object.keys(relationRowProblems(required, rows))).toEqual(["0.choices.0.content"]);
});

it("reads each stored row's rows through its relation, and none for a new row", async () => {
  mount({ title: "Q", kind: "blank" });
  expect(await screen.findByDisplayValue("yes")).toBeInTheDocument();
  expect(choicesAsked).toEqual(["1"]);
  // The enclosing row is a select blank: the choice's note shows (`when_parent`).
  expect(screen.getByDisplayValue("because")).toBeInTheDocument();
  // A text blank hides it.
  fireEvent.change(screen.getByLabelText(/^Mode/), { target: { value: JSON.stringify("text") } });
  expect(screen.queryByDisplayValue("because")).toBeNull();
  // A new blank starts without choices, and reads none.
  // (The blanks' button comes after the rows, whose own repeaters have theirs.)
  const adds = screen.getAllByRole("button", { name: /^Add/ });
  fireEvent.click(adds[adds.length - 1]!);
  expect(await screen.findAllByLabelText(/^Label/)).toHaveLength(2);
  expect(choicesAsked).toEqual(["1"]);
  // The change holds the stored blank's choices only where they changed.
  fireEvent.change(screen.getByDisplayValue("yes"), { target: { value: "yes!" } });
  const change = fromFormValue(blanks, form!.getValues().blanks!) as { update?: { key: string; data: Record<string, unknown> }[] };
  expect(change.update?.[0]).toEqual({ key: "1", data: { mode: "text", choices: { update: [{ key: "11", data: { content: "yes!" } }] } } });
});

it("says that hiding the rows removes the rows inside them too", async () => {
  mount({ title: "Q", kind: "blank" });
  await screen.findByDisplayValue("yes");
  fireEvent.change(screen.getByLabelText(/^Kind/), { target: { value: JSON.stringify("choice") } });
  // The blank's choices were read when the form opened: counted.
  expect(await screen.findByRole("status")).toHaveTextContent("Saving removes the 1 rows of Blanks you can see, and the 1 rows inside them.");
});

it("says in a row that hiding its rows removes them", async () => {
  mount({ title: "Q", kind: "blank" });
  await screen.findByDisplayValue("yes");
  // An open blank has no choices: its stored choice goes on save.
  fireEvent.change(screen.getByLabelText(/^Mode/), { target: { value: JSON.stringify("open") } });
  expect(await screen.findByRole("status")).toHaveTextContent("Saving removes the 1 rows of Choices you can see.");
});

describe("reactions on relationship rows (ADR 0044, part D)", () => {
  const stored = (): RelationState => ({
    rows: [
      { key: "1", values: { label: "First", mode: JSON.stringify("select"), hints: "", choices: "" }, changed: [], update: true, delete: true },
      { key: "2", values: { label: "Locked", mode: JSON.stringify("text"), hints: "", choices: "" }, changed: [], update: false, delete: false },
    ],
    deleted: [],
  });

  it("sends the rows the form holds: stored ones until changed, then the changes", () => {
    const values = { ...defaultValues(inputs, { title: "Q", kind: "blank" }), blanks: "" };
    expect(reactionState(inputs, values, () => stored()).blanks).toEqual([
      { key: "1", data: { label: "First", mode: "select", hints: null } },
      { key: "2", data: { label: "Locked", mode: "text", hints: null } },
    ]);
    const changed = { ...stored(), rows: [...stored().rows, { ref: "n7", values: { label: "New", mode: "", hints: "", choices: "" }, changed: [] }] };
    const state = reactionState(inputs, { ...values, blanks: JSON.stringify(changed) }, () => stored());
    expect((state.blanks as unknown[])[2]).toEqual({ ref: "n7", data: { label: "New", mode: null, hints: null } });
  });

  it("applies row patches by identity, never to rows the user may not change", () => {
    const values = { ...defaultValues(inputs, { title: "Q", kind: "blank" }), blanks: "" };
    const applied = applyPatches(inputs, values, [
      { op: "add_relation_row", field: "blanks", ref: "r1", data: { label: "Added" } },
      { op: "update_relation_row", field: "blanks", id: "1", data: { label: "First!" } },
      { op: "update_relation_row", field: "blanks", id: "2", data: { label: "Changed?" } },
      { op: "remove_relation_row", field: "blanks", id: "2" },
      { op: "flag_relation_row", field: "blanks", id: "r1", message: "check this" },
    ], () => stored());
    const state = JSON.parse(applied.values.blanks as string) as RelationState;
    expect(state.rows.map((r) => [r.key ?? r.ref, r.values.label])).toEqual([["1", "First!"], ["2", "Locked"], ["r1", "Added"]]);
    expect(state.rows[0]!.changed).toEqual(["label"]);
    expect(state.deleted).toEqual([]);
    // The warning stays with its row, by place once applied.
    expect(applied.flags.blanks).toEqual({ 2: ["check this"] });
    // A repeater whose rows the form does not hold (more than it reads: read-only) is neither sent nor patched.
    expect(reactionState(inputs, values, () => null)).not.toHaveProperty("blanks");
    const locked = applyPatches(inputs, values, [{ op: "add_relation_row", field: "blanks", ref: "r1", data: { label: "x" } }], () => null);
    expect(locked.values).not.toHaveProperty("blanks");
    // A removal the user may make.
    const removed = applyPatches(inputs, values, [{ op: "remove_relation_row", field: "blanks", id: "1" }], () => stored());
    expect((JSON.parse(removed.values.blanks as string) as RelationState).deleted).toEqual(["1"]);
  });
});

it("shows rows of rows on the view page, each read through its row's relation", async () => {
  server.use(
    http.get("*/admin/api/resources/questions/records/:key/relations/blanks", () =>
      HttpResponse.json({ data: [{ key: "1", abilities: {}, data: { label: "First" } }], meta: { total: 1, page: 1, perPage: 100, lastPage: 1 } })),
    http.get("*/admin/api/resources/question_blanks/records/:key/relations/choices", ({ params }) => {
      choicesAsked.push(String(params.key));
      return HttpResponse.json({
        data: [{ key: "11", abilities: {}, data: { content: "yes" } }],
        meta: { total: 3, page: 1, perPage: 1, lastPage: 3 },
      });
    }),
  );
  const infolist = {
    columns: 1,
    components: [{
      type: "repeatable_entry", name: "blanks", label: "Blanks", valueType: "rows", columnSpan: 1,
      rows: { resource: "question_blanks", relation: "blanks", limit: 100 },
      components: [
        { type: "text_entry", name: "label", label: "Label", valueType: "string", columnSpan: 1 },
        {
          type: "repeatable_entry", name: "choices", label: "Choices", valueType: "rows", columnSpan: 1,
          rows: { resource: "question_options", relation: "choices", limit: 1 },
          components: [{ type: "text_entry", name: "content", label: "Content", valueType: "string", columnSpan: 1 }],
        },
      ],
    }],
  } as unknown as InfolistSchema;
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const session = createSession({ basePath: "", apiPath: "http://localhost/admin/api" }, client, () => {});
  const history = createMemoryHistory({ initialEntries: ["/r/questions/records/7"] });
  const router = createRootRoute({ component: () => (
    <SessionContext.Provider value={session}><QueryClientProvider client={client}>
      <InfolistRenderer infolist={infolist} slug="questions" recordKey="7" data={{ title: "Q" }} />
    </QueryClientProvider></SessionContext.Provider>
  ) });
  render(<RouterProvider router={createRouter({ routeTree: router, history })} />);
  // Rows of rows are read when asked: none on open.
  fireEvent.click(await screen.findByRole("button", { name: "Show Choices" }));
  expect(await screen.findByText("yes")).toBeInTheDocument();
  expect(choicesAsked).toEqual(["1"]);
  // More choices than shown: the blank's page has them all.
  expect(screen.getByRole("link", { name: "All 3 rows" })).toHaveAttribute("href", "/r/question_blanks/records/1#relation-choices");
});
