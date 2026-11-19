import { fireEvent, render, screen } from "@testing-library/react";
import { useForm } from "react-hook-form";
import type { ComponentSchema, InputSchema, LayoutSchema } from "@/api/types";
import { Components } from "@/renderers/form";
import { InfolistRenderer } from "@/renderers/infolist";
import { infolistOf } from "./view";
import { formConditions, hiddenFields } from "@/schema/conditions";
import { type FormValues, defaultValues, hiddenNames, inputsOf, payloadOf } from "@/schema/form";
import { editResolver } from "@/schema/resolver";
import { boundInput, boundValue } from "@/pages/relations";
import cases from "../../fixtures/conditions/cases.json";

// The cases the Rust engine runs too (engine/conditions.rs): both sides hide the same fields.
describe("shared condition cases", () => {
  for (const form of cases.forms) {
    it(form.name, () => {
      const conditions = formConditions(form.components as unknown as ComponentSchema[]);
      // A repeater's row reading the record's form (ADR 0044).
      const kinds = "root" in form && form.root ? new Map(Object.entries(form.root.kinds)) : undefined;
      for (const c of form.cases) {
        const outer = kinds && "root" in c ? { root: { values: c.root as Record<string, unknown>, kinds } } : {};
        expect([...hiddenFields(conditions, c.values, outer)].sort(), JSON.stringify(c.values)).toEqual(c.hidden);
      }
    });
  }
});

const input = (name: string, extra: Partial<InputSchema> = {}): InputSchema => ({
  name, label: name, type: "text_input", valueType: "string", nullable: true,
  disabled: false, rules: [], columnSpan: 1, ...extra,
});

const components: ComponentSchema[] = [
  input("link_type", { type: "select", options: [{ value: "internal", label: "Internal" }, { value: "external", label: "External" }] }),
  input("url", { label: "URL", rules: [{ rule: "required" }], shownWhen: { op: "eq", name: "link_type", value: "external" } }),
  input("on_sale", { type: "toggle", valueType: "bool", nullable: false }),
  {
    type: "section", label: "Sale", columns: 1, shownWhen: { op: "is_true", name: "on_sale" },
    components: [input("discount", { label: "Discount", type: "number_input", valueType: "i32" })],
  } as LayoutSchema,
];

let form: ReturnType<typeof useForm<FormValues>> | undefined;
function Harness({ data }: { data: Record<string, unknown> }) {
  form = useForm<FormValues>({ defaultValues: defaultValues(inputsOf(components), data) });
  return <Components components={components} ctx={{ slug: "offers", control: form.control, errors: {} }} />;
}

it("shows and hides inputs and sections as the values change, without losing drafts", () => {
  render(<Harness data={{ link_type: "internal", on_sale: false }} />);
  expect(screen.queryByLabelText(/^URL/)).toBeNull();
  expect(screen.queryByText("Sale")).toBeNull();

  // Choosing "External" shows the URL at once (no request).
  fireEvent.change(screen.getByLabelText(/^link_type/), { target: { value: "\"external\"" } });
  const url = screen.getByLabelText(/^URL/);
  fireEvent.change(url, { target: { value: "https://draft" } });
  // Hidden again, then shown: the draft is still there.
  fireEvent.change(screen.getByLabelText(/^link_type/), { target: { value: "\"internal\"" } });
  expect(screen.queryByLabelText(/^URL/)).toBeNull();
  fireEvent.change(screen.getByLabelText(/^link_type/), { target: { value: "\"external\"" } });
  expect(screen.getByLabelText(/^URL/)).toHaveValue("https://draft");

  fireEvent.click(screen.getByRole("switch", { name: /^on_sale/ }));
  expect(screen.getByText("Sale")).toBeInTheDocument();
  expect(screen.getByLabelText(/^Discount/)).toBeInTheDocument();
});

it("never sends nor checks hidden inputs", async () => {
  const inputs = inputsOf(components);
  const values = defaultValues(inputs, { link_type: "internal", url: null, on_sale: false, discount: 5 });
  const hidden = hiddenNames(components, values);
  expect([...hidden].sort()).toEqual(["discount", "url"]);
  expect(payloadOf(inputs, { ...values, url: "https://kept" }, undefined, hidden)).toEqual({ link_type: "internal", on_sale: false });

  // `url` is required only when shown.
  const resolver = editResolver(inputs, false, () => true, components);
  const check = (v: FormValues) => resolver(v, undefined, { fields: {}, shouldUseNativeValidation: false } as never);
  expect((await check(values)).errors).toEqual({});
  const external = { ...values, link_type: "\"external\"" };
  expect(Object.keys((await check(external)).errors)).toEqual(["url"]);
});

it("leaves out a tab whose condition fails, on forms and view pages", () => {
  const tabs: ComponentSchema[] = [
    input("on_sale", { label: "On sale", type: "toggle", valueType: "bool", nullable: false }),
    {
      type: "tabs", columns: 1, components: [
        { type: "tab", id: "main", label: "Main", columns: 1, components: [input("title", { label: "Title" })] },
        {
          type: "tab", id: "sale", label: "Sale", columns: 1, shownWhen: { op: "is_true", name: "on_sale" },
          components: [input("discount", { label: "Discount", type: "number_input", valueType: "i32" })],
        },
      ],
    } as LayoutSchema,
  ];
  function Tabs() {
    form = useForm<FormValues>({ defaultValues: defaultValues(inputsOf(tabs), { on_sale: false }) });
    return <Components components={tabs} ctx={{ slug: "offers", control: form.control, errors: {} }} />;
  }
  const { unmount } = render(<Tabs />);
  expect(screen.getByRole("tab", { name: "Main" })).toBeInTheDocument();
  expect(screen.queryByRole("tab", { name: "Sale" })).toBeNull();
  fireEvent.click(screen.getByRole("switch", { name: /^On sale/ }));
  expect(screen.getByRole("tab", { name: "Sale" })).toBeInTheDocument();
  expect([...hiddenNames(tabs, { on_sale: false, title: "", discount: "" })]).toEqual(["discount"]);
  unmount();

  render(<InfolistRenderer infolist={infolistOf(tabs)} form={tabs} data={{ on_sale: false, title: "T", discount: 5 }} slug="offers" recordKey="1" />);
  expect(screen.queryByRole("tab", { name: "Sale" })).toBeNull();
});

it("shows the view page by the stored values", () => {
  render(<InfolistRenderer infolist={infolistOf(components)} form={components} data={{ link_type: "internal", url: "https://old", on_sale: true, discount: 3 }} slug="offers" recordKey="1" />);
  expect(screen.queryByText("URL")).toBeNull();
  expect(screen.getByText("Discount")).toBeInTheDocument();
  expect(screen.getByText("3")).toBeInTheDocument();
});

it("keeps the open tab when a tab before it is shown or hidden", () => {
  const tabs: ComponentSchema[] = [
    input("on_sale", { label: "On sale", type: "toggle", valueType: "bool", nullable: false }),
    {
      type: "tabs", columns: 1, components: [
        {
          type: "tab", id: "sale", label: "Sale", columns: 1, shownWhen: { op: "is_true", name: "on_sale" },
          components: [input("discount", { label: "Discount", type: "number_input", valueType: "i32" })],
        },
        { type: "tab", id: "main", label: "Main", columns: 1, components: [input("title", { label: "Title" })] },
        { type: "tab", id: "more", label: "More", columns: 1, components: [input("note", { label: "Note" })] },
      ],
    } as LayoutSchema,
  ];
  function Tabs() {
    form = useForm<FormValues>({ defaultValues: defaultValues(inputsOf(tabs), { on_sale: false }) });
    return <Components components={tabs} ctx={{ slug: "offers", control: form.control, errors: {} }} />;
  }
  render(<Tabs />);
  fireEvent.click(screen.getByRole("tab", { name: "More" }));
  expect(screen.getByRole("tab", { name: "More" })).toHaveAttribute("aria-selected", "true");
  // "Sale" appears before it: "More" stays open.
  fireEvent.click(screen.getByRole("switch", { name: /^On sale/ }));
  expect(screen.getByRole("tab", { name: "More" })).toHaveAttribute("aria-selected", "true");
  // The open tab hidden: the first shown one opens; shown again, it is open again.
  fireEvent.click(screen.getByRole("tab", { name: "Sale" }));
  fireEvent.click(screen.getByRole("switch", { name: /^On sale/ }));
  expect(screen.getByRole("tab", { name: "Main" })).toHaveAttribute("aria-selected", "true");
  fireEvent.click(screen.getByRole("switch", { name: /^On sale/ }));
  expect(screen.getByRole("tab", { name: "Sale" })).toHaveAttribute("aria-selected", "true");
});

it("reads a key bound by a relation in the conditions, without sending it", () => {
  const components: ComponentSchema[] = [
    input("team_id", { type: "select", valueType: "i32" }),
    input("seat", { shownWhen: { op: "filled", name: "team_id" } }),
  ];
  const bound = boundInput(components, "team_id", boundValue("i32", "7"));
  const values = defaultValues(inputsOf(bound));
  expect(hiddenNames(bound, values).has("seat")).toBe(false);
  expect(hiddenNames(components, defaultValues(inputsOf(components))).has("seat")).toBe(true);
  const payload = payloadOf(inputsOf(bound), { ...values, seat: "A1" }, undefined, hiddenNames(bound, values));
  expect(payload).toEqual({ seat: "A1" });
});
