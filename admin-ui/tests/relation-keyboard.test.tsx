import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { http, HttpResponse } from "msw";
import { setupServer } from "msw/node";
import { useState } from "react";
import { useForm } from "react-hook-form";
import { SessionContext } from "@/api/context";
import type { InputSchema } from "@/api/types";
import { createSession } from "@/app/session";
import { Components } from "@/renderers/form";
import { RelationSelect } from "@/renderers/relation-select";
import { defaultValues, type FormValues } from "@/schema/form";

const server = setupServer(http.get("*/admin/api/resources/:slug/fields/:field/options", () => HttpResponse.json({ data: [
  { value: "9007199254740993", label: "Alpha" }, { value: "2", label: "Beta" },
] })));
beforeAll(() => server.listen({ onUnhandledRequest: "error" }));
afterAll(() => server.close());

function Harness({ initial = "", onChange = () => {} }: { initial?: string; onChange?: (value: string) => void }) {
  const [value, setValue] = useState(initial);
  return <>
    <label htmlFor="category">Category</label>
    <p id="help">Choose a category.</p>
    <p id="error">A category is required.</p>
    <RelationSelect id="category" slug="posts" field="category" record="7" value={value}
      invalid describedBy="help error" onChange={(next) => { setValue(next); onChange(next); }} />
    <button type="button">After select</button>
  </>;
}

function mount(children: React.ReactNode) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const session = createSession({ basePath: "/admin", apiPath: "http://localhost/admin/api" }, client, () => {});
  return render(<SessionContext.Provider value={session}><QueryClientProvider client={client}>
    {children}
  </QueryClientProvider></SessionContext.Provider>);
}

it("opens with arrows, moves the active option, chooses with Enter and restores trigger focus", async () => {
  const change = vi.fn();
  mount(<Harness onChange={change} />);
  const trigger = screen.getByRole("combobox", { name: "Category" });
  expect(trigger).toHaveAttribute("aria-invalid", "true");
  expect(trigger).toHaveAttribute("aria-describedby", "help error");
  trigger.focus();
  await userEvent.keyboard("{ArrowDown}");
  const search = screen.getByRole("combobox", { name: "Search options" });
  expect(search).toHaveFocus();
  await screen.findByRole("option", { name: "Alpha" });
  await waitFor(() => expect(search).toHaveAttribute("aria-activedescendant", screen.getByRole("option", { name: "Alpha" }).id));
  await userEvent.keyboard("{ArrowDown}");
  expect(search).toHaveAttribute("aria-activedescendant", screen.getByRole("option", { name: "Beta" }).id);
  await userEvent.keyboard("{ArrowUp}{Enter}");
  expect(change).toHaveBeenCalledWith('"9007199254740993"');
  expect(trigger).toHaveFocus();
  expect(trigger).toHaveAttribute("aria-expanded", "false");
  expect(screen.queryByRole("listbox")).toBeNull();
});

it("Escape cancels without changing the key and Clear participates in keyboard selection", async () => {
  const change = vi.fn();
  mount(<Harness initial='"2"' onChange={change} />);
  const trigger = screen.getByRole("combobox", { name: "Category" });
  await userEvent.click(trigger);
  await userEvent.keyboard("{Escape}");
  expect(change).not.toHaveBeenCalled();
  expect(trigger).toHaveFocus();
  await userEvent.click(trigger);
  expect(screen.getByRole("option", { name: "Clear" })).toBeInTheDocument();
  await userEvent.keyboard("{Enter}");
  expect(change).toHaveBeenCalledWith("");
  expect(trigger).toHaveFocus();
});

it("Tab leaves the popup without selecting or trapping focus", async () => {
  const change = vi.fn();
  mount(<Harness onChange={change} />);
  await userEvent.click(screen.getByRole("combobox", { name: "Category" }));
  await screen.findByRole("option", { name: "Alpha" });
  await userEvent.tab();
  expect(screen.getByRole("button", { name: "After select" })).toHaveFocus();
  expect(screen.queryByRole("listbox")).toBeNull();
  expect(change).not.toHaveBeenCalled();
});

it.each([
  ["native composition", { isComposing: true }],
  ["legacy composition key", { isComposing: false, keyCode: 229 }],
])("leaves IME navigation and confirmation untouched during %s", async (_name, composition) => {
  const change = vi.fn();
  mount(<Harness onChange={change} />);
  const trigger = screen.getByRole("combobox", { name: "Category" });
  trigger.focus();
  expect(fireEvent.keyDown(trigger, { key: "ArrowDown", ...composition })).toBe(true);
  expect(trigger).toHaveAttribute("aria-expanded", "false");
  await userEvent.click(trigger);
  const search = screen.getByRole("combobox", { name: "Search options" });
  await screen.findByRole("option", { name: "Alpha" });
  await waitFor(() => expect(search).toHaveAttribute("aria-activedescendant", screen.getByRole("option", { name: "Alpha" }).id));
  const active = search.getAttribute("aria-activedescendant");
  for (const key of ["ArrowDown", "ArrowUp", "Enter", "Escape"]) {
    // true means no preventDefault: the IME may navigate/commit its own candidate.
    expect(fireEvent.keyDown(search, { key, ...composition })).toBe(true);
    expect(search).toHaveAttribute("aria-activedescendant", active);
    expect(search).toHaveFocus();
    expect(trigger).toHaveAttribute("aria-expanded", "true");
  }
  expect(change).not.toHaveBeenCalled();
  // Normal keyboard selection still works after composition ends.
  fireEvent.keyDown(search, { key: "ArrowDown", isComposing: false, keyCode: 40 });
  expect(search).toHaveAttribute("aria-activedescendant", screen.getByRole("option", { name: "Beta" }).id);
  fireEvent.keyDown(search, { key: "Enter", isComposing: false, keyCode: 13 });
  expect(change).toHaveBeenCalledWith('"2"');
  expect(trigger).toHaveFocus();
});

const field = (name: string, type: InputSchema["type"], extra: Partial<InputSchema> = {}): InputSchema => ({
  name, label: name, type, valueType: "string", nullable: false, disabled: false,
  rules: [], columnSpan: 1, help: `${name} help`, ...extra,
});
const fields = [
  field("text", "text_input"), field("notes", "textarea"), field("number", "number_input"),
  field("date", "date_picker"), field("instant", "date_time_picker", { valueType: "instant" }),
  field("toggle", "toggle", { valueType: "bool" }), field("check", "checkbox", { valueType: "bool" }),
  field("tri", "toggle", { valueType: "bool", nullable: true }),
  field("list", "checkbox_list", { valueType: "json", options: [{ value: "one", label: "One" }] }),
  field("select", "select", { options: [{ value: "one", label: "One" }] }),
  field("relation", "select", { relationship: { resource: "categories" } }),
];

function FieldsHarness() {
  const form = useForm<FormValues>({ defaultValues: defaultValues(fields) });
  return <Components components={fields} ctx={{ slug: "posts", control: form.control,
    errors: Object.fromEntries(fields.map((input) => [input.name, { message: `${input.name} error`, type: "manual" }])),
  }} />;
}

it("links help and errors and exposes invalid state for every form widget", () => {
  const { container } = mount(<FieldsHarness />);
  for (const input of fields) {
    const widget = container.querySelector(`#field-${input.name}`)!;
    expect(widget).toHaveAttribute("aria-invalid", "true");
    expect(widget).toHaveAttribute("aria-describedby", `field-${input.name}-help field-${input.name}-error`);
    expect(document.getElementById(`field-${input.name}-help`)).toHaveTextContent(`${input.name} help`);
    expect(document.getElementById(`field-${input.name}-error`)).toHaveTextContent(`${input.name} error`);
  }
  expect(screen.getByRole("checkbox", { name: "One" })).toHaveAttribute("aria-invalid", "true");
});
