import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import { http, HttpResponse } from "msw";
import { setupServer } from "msw/node";
import type * as React from "react";
import { useForm } from "react-hook-form";
import { SessionContext } from "@/api/context";
import type { ComponentSchema, Envelope, Page, ResourceSchema } from "@/api/types";
import { createSession } from "@/app/session";
import { Components } from "@/renderers/form";
import { Cell } from "@/renderers/table";
import { compatibility } from "@/components/states";
import { type FormValues, defaultValues, inputsOf } from "@/schema/form";
import create from "../../fixtures/contract/kitchen-sink.schema.create.json";
import edit from "../../fixtures/contract/kitchen-sink.schema.edit.json";
import view from "../../fixtures/contract/kitchen-sink.schema.view.json";
import records from "../../fixtures/contract/kitchen-sink.records.json";
import posts from "../../fixtures/contract/posts.schema.json";

const server = setupServer(
  http.get("*/admin/api/resources/:slug/fields/:field/options", () =>
    HttpResponse.json({ data: [{ value: 1, label: "Tools" }] }),
  ),
);
beforeAll(() => server.listen({ onUnhandledRequest: "error" }));
afterAll(() => server.close());

function Providers({ children }: { children: React.ReactNode }) {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const session = createSession({ basePath: "/admin", apiPath: "http://localhost/admin/api" }, queryClient, () => {});
  return (
    <SessionContext.Provider value={session}>
      <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
    </SessionContext.Provider>
  );
}

function FormHarness({ schema, data, readOnly }: { schema: ResourceSchema; data?: Record<string, unknown>; readOnly?: boolean }) {
  const inputs = inputsOf(schema.form.components);
  const form = useForm<FormValues>({ defaultValues: defaultValues(inputs, data) });
  return (
    <Components
      components={schema.form.components}
      ctx={{ slug: schema.resource.slug, record: data ? "1" : undefined, control: form.control, errors: {}, readOnly }}
    />
  );
}

const record = (records as unknown as Page).data[0]!;

describe.each([
  ["create", create],
  ["edit", edit],
  ["view", view],
  ["posts", posts],
])("contract schema %s", (_name, fixture) => {
  const schema = fixture as unknown as ResourceSchema;

  it("is supported by this bundle", () => {
    expect(compatibility(schema)).toEqual({ readOnly: false, outdated: false });
  });

  it("renders every input without fallback", async () => {
    const { container } = render(
      <Providers>
        <FormHarness schema={schema} data={schema.operation === "create" ? undefined : record.data} readOnly={schema.operation === "view"} />
      </Providers>,
    );
    for (const input of inputsOf(schema.form.components)) {
      if (input.type === "hidden") continue;
      expect(screen.getAllByText(input.label, { exact: false }).length).toBeGreaterThan(0);
    }
    expect(container.querySelector("[data-fallback]")).toBeNull();
    if (schema.resource.slug === "gadgets" && schema.operation !== "create") {
      expect(await screen.findByText("Tools")).toBeInTheDocument();
      expect(screen.getByDisplayValue("9007199254740993")).toBeInTheDocument();
      expect(screen.getByDisplayValue("10.50")).toBeInTheDocument();
    }
  });
});

it("renders unknown components as a read-only fallback", () => {
  const schema = structuredClone(create) as unknown as ResourceSchema;
  const notes = inputsOf(schema.form.components).find((i) => i.name === "notes")!;
  const unknown = { ...notes, type: "rich_editor" };
  schema.form.components = [unknown as ComponentSchema];
  const { container } = render(
    <Providers>
      <FormHarness schema={schema} />
    </Providers>,
  );
  const fallback = container.querySelector("[data-fallback]") as HTMLInputElement;
  expect(fallback).not.toBeNull();
  expect(fallback.disabled).toBe(true);
});

it("renders every table column of the contract", async () => {
  const schema = view as unknown as ResourceSchema;
  render(
    <table>
      <tbody>
        <tr>
          {schema.table.columns.map((c) => (
            <td key={c.name}>
              <Cell column={c} record={record as Envelope} />
            </td>
          ))}
        </tr>
      </tbody>
    </table>,
  );
  expect(screen.getByText("Widget")).toBeInTheDocument();
  expect(screen.getByText("live")).toBeInTheDocument();
  expect(screen.getByText("Tools")).toBeInTheDocument();
  expect(screen.getByText("10.50")).toBeInTheDocument();
  expect(screen.getByText("9007199254740993")).toBeInTheDocument();
  expect(screen.getByText("2026-10-12")).toBeInTheDocument();
  expect(screen.getByLabelText("Yes")).toBeInTheDocument();
});
