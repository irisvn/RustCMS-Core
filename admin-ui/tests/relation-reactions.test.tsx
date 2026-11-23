// Form reactions on relationship rows (ADR 0044, part D), from the server's
// contract (fixtures/contract/repeater-relation.*): the rows sent as state
// (tagged by identity, the user's unsaved edits included), the four row
// patches applied by identity, and a warning shown under its row.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, within } from "@testing-library/react";
import { http, HttpResponse } from "msw";
import { setupServer } from "msw/node";
import { useForm } from "react-hook-form";
import { SessionContext } from "@/api/context";
import type { ComponentSchema, Envelope, FormPatch, RepeaterSchema } from "@/api/types";
import { createSession } from "@/app/session";
import { Components } from "@/renderers/form";
import { type FormValues, defaultValues, inputsOf, payloadOf } from "@/schema/form";
import { applyPatches, reactionState } from "@/schema/reactions";
import { type RelationState, parseState, stateOf } from "@/schema/relation-rows";
import schema from "../../fixtures/contract/repeater-relation.schema.edit.json";
import rows from "../../fixtures/contract/repeater-relation.rows.json";

const components = schema.form.components as unknown as ComponentSchema[];
const inputs = inputsOf(components);
const lines = inputs.find((i) => i.name === "lines")!;
const repeater = lines.repeater as RepeaterSchema;
const records = rows.data as unknown as Envelope[];
const stored = () => stateOf(repeater, records);
const values = (): FormValues => defaultValues(inputs, { number: "A-1" });

describe("the state", () => {
  it("sends the stored rows by key, then the user's edits and new rows by ref", () => {
    expect(reactionState(inputs, values(), () => stored()).lines).toEqual([
      { key: "1", data: { product: "a", quantity: 1 } },
      { key: "2", data: { product: "b", quantity: 7 } },
      { key: "3", data: { product: "c", quantity: 3 } },
    ]);
    const edited: RelationState = { ...stored(), rows: [...stored().rows.slice(1), { ref: "n9", values: { product: "d", quantity: "x" }, changed: [] }], deleted: ["1"] };
    edited.rows[0] = { ...edited.rows[0]!, values: { ...edited.rows[0]!.values, quantity: "20" }, changed: ["quantity"] };
    expect(reactionState(inputs, { ...values(), lines: JSON.stringify(edited) }, () => stored()).lines).toEqual([
      { key: "2", data: { product: "b", quantity: 20 } },
      { key: "3", data: { product: "c", quantity: 3 } },
      // Half typed: left out.
      { ref: "n9", data: { product: "d" } },
    ]);
  });

  it("leaves out rows the form does not hold (not read, or more than it reads)", () => {
    expect(reactionState(inputs, values(), () => null)).not.toHaveProperty("lines");
  });
});

describe("row patches", () => {
  it("apply by tagged identity, new rows getting the form's own references", () => {
    const patches: FormPatch[] = [
      { op: "update_relation_row", field: "lines", id: { key: "2" }, data: { quantity: 5 } },
      { op: "add_relation_row", field: "lines", ref: "n1", data: { product: "d", quantity: 4 } },
      // The answer's `n1` is the row it added, not a key `n1`.
      { op: "update_relation_row", field: "lines", id: { ref: "n1" }, data: { product: "e" } },
      { op: "update_relation_row", field: "lines", id: { key: "n1" }, data: { product: "never" } },
      { op: "remove_relation_row", field: "lines", id: { key: "1" } },
      { op: "flag_relation_row", field: "lines", id: { ref: "n1" }, message: "Check this one" },
      { op: "flag_relation_row", field: "lines", id: { key: "3" }, message: "And this" },
      { op: "remove_relation_row", field: "lines", id: { ref: "n404" } },
    ];
    const applied = applyPatches(inputs, values(), patches, () => stored());
    const state = parseState(applied.values.lines!)!;
    expect(state.rows.map((r) => [r.key ?? "new", r.values.product, r.values.quantity])).toEqual([
      ["2", "b", "5"],
      ["3", "c", "3"],
      ["new", "e", "4"],
    ]);
    // A stored row sends only what changed; the new row has its own reference.
    expect(state.rows[0]!.changed).toEqual(["quantity"]);
    expect(state.rows[2]!.ref).toMatch(/^n\d+$/);
    expect(state.deleted).toEqual(["1"]);
    // Warnings stay with their rows, by place once applied.
    expect(applied.flags.lines).toEqual({ 1: ["And this"], 2: ["Check this one"] });
    const change = payloadOf(inputs, { ...values(), lines: applied.values.lines! }).lines as Record<string, unknown>;
    expect(change).toMatchObject({ update: [{ key: "2", data: { quantity: 5 } }], delete: ["1"], create: [{ data: { product: "e", quantity: 4 } }] });
  });

  it("map the answer's references per repeater", () => {
    // A second relationship repeater on the same rows' schema: both may add an `n1`.
    const extras = { ...lines, name: "extras", repeater: { ...repeater, relationship: { ...repeater.relationship!, relation: "extras" } } };
    const both = [...inputs, extras];
    const applied = applyPatches(both, { ...values(), extras: "" }, [
      { op: "add_relation_row", field: "lines", ref: "n1", data: { product: "line" } },
      { op: "add_relation_row", field: "extras", ref: "n1", data: { product: "extra" } },
      { op: "update_relation_row", field: "lines", id: { ref: "n1" }, data: { product: "line, renamed" } },
      { op: "update_relation_row", field: "extras", id: { ref: "n1" }, data: { quantity: 2 } },
    ], () => ({ rows: [], deleted: [] }));
    const added = (name: string) => parseState(applied.values[name]!)!.rows.map((r) => [r.values.product, r.values.quantity]);
    expect(added("lines")).toEqual([["line, renamed", ""]]);
    expect(added("extras")).toEqual([["extra", "2"]]);
  });

  it("leave alone rows the user may not change and repeaters the form does not hold", () => {
    const locked = () => ({ ...stored(), rows: stored().rows.map((r) => ({ ...r, update: false, delete: false })) });
    const applied = applyPatches(inputs, values(), [
      { op: "update_relation_row", field: "lines", id: { key: "1" }, data: { quantity: 9 } },
      { op: "remove_relation_row", field: "lines", id: { key: "2" } },
    ], locked);
    expect(applied.values).toEqual({});
    expect(applyPatches(inputs, values(), [{ op: "add_relation_row", field: "lines", data: { product: "x" } }], () => null).values).toEqual({});
  });
});

const server = setupServer(
  http.get("*/admin/api/resources/orders/records/:key/relations/lines", () => HttpResponse.json(rows)),
);
beforeAll(() => server.listen({ onUnhandledRequest: "error" }));
afterAll(() => server.close());

function Harness() {
  const form = useForm<FormValues>({ defaultValues: values() });
  return (
    <Components components={components}
      ctx={{ slug: "orders", record: "1", control: form.control, errors: form.formState.errors, rowFlags: { lines: { 1: ["Low stock"] } } }} />
  );
}

it("shows a reaction's warning under its row", async () => {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const session = createSession({ basePath: "", apiPath: "http://localhost/admin/api" }, client, () => {});
  render(<SessionContext.Provider value={session}><QueryClientProvider client={client}><Harness /></QueryClientProvider></SessionContext.Provider>);
  await screen.findByText("a");
  const items = screen.getAllByRole("listitem");
  expect(within(items[1]!).getByRole("status")).toHaveTextContent("Low stock");
  expect(within(items[0]!).queryByRole("status")).toBeNull();
});
