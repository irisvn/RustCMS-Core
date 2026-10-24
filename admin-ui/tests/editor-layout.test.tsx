// The editor layout (vocabulary 11): a split with a sticky aside, sections
// that fold or are compact, text inputs with a fixed prefix or suffix, and the
// page width of a record's form.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type * as React from "react";
import { useForm } from "react-hook-form";
import { SessionContext } from "@/api/context";
import type { ComponentSchema, InputSchema, LayoutSchema } from "@/api/types";
import { createSession } from "@/app/session";
import { Components, ReadOnlyComponents, formWidthClass } from "@/renderers/form";
import { type FormValues, defaultValues, inputsOf } from "@/schema/form";

const text = (name: string, label: string, more: Partial<InputSchema> = {}): InputSchema => ({
  type: "text_input", name, label, valueType: "string", nullable: true, disabled: false, writeOnly: false,
  rules: [], columnSpan: 1, options: [], ...more,
});
const layout = (type: LayoutSchema["type"], components: ComponentSchema[], more: Partial<LayoutSchema> = {}): LayoutSchema => ({
  type, columns: 1, components, ...more,
});
const editor = (asideFirst = false): ComponentSchema[] => [
  layout("split", [
    layout("pane", [
      text("title", "Title"),
      text("slug", "Slug", { prefix: "/posts/", suffix: ".html", help: "The address" }),
      layout("section", [text("meta_title", "Meta title")], { label: "SEO", collapsed: true }),
    ], { id: "main" }),
    layout("pane", [layout("section", [text("status", "Status")], { label: "Publishing", compact: true })], { id: "aside" }),
  ], { asideFirst }),
];

function Providers({ children }: { children: React.ReactNode }) {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const session = createSession({ basePath: "/admin", apiPath: "http://localhost/admin/api" }, queryClient, () => {});
  return (
    <SessionContext.Provider value={session}>
      <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
    </SessionContext.Provider>
  );
}

function Harness({ components, errors = {}, attempt = 0 }: { components: ComponentSchema[]; errors?: Record<string, { type: string; message: string }>; attempt?: number }) {
  const form = useForm<FormValues>({ defaultValues: defaultValues(inputsOf(components)) });
  return <Components components={components} ctx={{ slug: "posts", control: form.control, errors, submitCount: attempt }} />;
}

it("lays the main column and the aside out side by side from lg, the aside sticky", () => {
  const { container } = render(<Providers><Harness components={editor()} /></Providers>);
  const aside = container.querySelector("aside")!;
  expect(aside).toHaveClass("lg:sticky", "lg:top-20", "lg:self-start");
  expect(aside.parentElement).toHaveClass("lg:grid-cols-[minmax(0,1fr)_20rem]");
  expect(within(aside).getByLabelText("Status")).toBeInTheDocument();
  // After the main column on narrow screens, unless asked otherwise.
  expect(aside).not.toHaveClass("order-first");
  expect(container.querySelector("[style]")).toBeNull();
});

it("puts the aside first on narrow screens when asked", () => {
  const { container } = render(<Providers><Harness components={editor(true)} /></Providers>);
  expect(container.querySelector("aside")).toHaveClass("order-first", "lg:order-none");
});

it("folds a section with a button, keeps its inputs, and opens it on an error", async () => {
  const { rerender } = render(<Providers><Harness components={editor()} /></Providers>);
  const toggle = screen.getByRole("button", { name: "SEO" });
  expect(toggle).toHaveAttribute("aria-expanded", "false");
  // Folded: hidden, still in the form.
  expect(screen.getByLabelText("Meta title")).not.toBeVisible();
  await userEvent.click(toggle);
  expect(toggle).toHaveAttribute("aria-expanded", "true");
  expect(screen.getByLabelText("Meta title")).toBeVisible();
  await userEvent.click(toggle);
  expect(screen.getByLabelText("Meta title")).not.toBeVisible();
  // A failed save with an error inside: opened.
  rerender(<Providers><Harness components={editor()} errors={{ meta_title: { type: "server", message: "is too long" } }} attempt={1} /></Providers>);
  expect(screen.getByRole("button", { name: "SEO" })).toHaveAttribute("aria-expanded", "true");
});

it("draws a compact section tighter", () => {
  const { container } = render(<Providers><Harness components={editor()} /></Providers>);
  const publishing = within(container.querySelector("aside")!).getByRole("group", { name: "Publishing" });
  expect(publishing).toHaveClass("p-4");
});

it("shows a prefix and a suffix inside the input, never in the value, read with it", async () => {
  render(<Providers><Harness components={editor()} /></Providers>);
  const slug = screen.getByLabelText("Slug");
  await userEvent.type(slug, "hello");
  expect(slug).toHaveValue("hello");
  const described = (slug.getAttribute("aria-describedby") ?? "").split(" ");
  const texts = described.map((id) => document.getElementById(id)?.textContent);
  expect(texts).toEqual(expect.arrayContaining(["/posts/", ".html"]));
});

it("lays a viewed record out the same way", () => {
  const { container } = render(
    <Providers><ReadOnlyComponents components={editor()} data={{ title: "Hi", status: "draft" }} slug="posts" record="1" /></Providers>,
  );
  expect(container.querySelector("aside")).toHaveClass("lg:sticky");
  expect(within(container.querySelector("aside")!).getByText("draft")).toBeInTheDocument();
});

it("maps form widths to static classes, unknown ones to the default", () => {
  expect([formWidthClass(undefined), formWidthClass("default"), formWidthClass("wide"), formWidthClass("full"), formWidthClass("huge")])
    .toEqual(["max-w-4xl", "max-w-4xl", "max-w-6xl", "max-w-none", "max-w-4xl"]);
});
