// Options computed on the server (ADR 0044, part E): which inputs ask and
// what they depend on, answers shown in place of the declared options, the
// form asking when it opens and once a dependency's change pauses, applying
// only the latest answer, keeping the declared options when asking fails,
// and checked values no longer among the options. Schemas and answers from
// the server's contract.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { RouterProvider, createMemoryHistory } from "@tanstack/react-router";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { http, HttpResponse, delay } from "msw";
import { setupServer } from "msw/node";
import { useForm } from "react-hook-form";
import { SessionContext } from "@/api/context";
import type { FormOptionsRequest, InputSchema, ResourceSchema, SelectOption } from "@/api/types";
import { createAppRouter } from "@/app/router";
import { createSession } from "@/app/session";
import { stayOnPurpose } from "@/lib/leave";
import { Components } from "@/renderers/form";
import { type FormValues, defaultValues, inputsOf } from "@/schema/form";
import { optionDependents, optionInputs, withOptions } from "@/schema/options";
import schema from "../../fixtures/contract/options-fn.schema.create.json";
import answered from "../../fixtures/contract/options-fn.options.json";
import failed from "../../fixtures/contract/options-fn.failed.json";

const resource = schema as unknown as ResourceSchema;
const components = resource.form.components;
const inputs = inputsOf(components);

describe("inputs with computed options", () => {
  it("ask for them and depend on the inputs the server named", () => {
    // Not `rank`: the contract's super admin may not read it (write-only).
    expect(optionInputs(inputs).map((i) => i.name)).toEqual(["topic", "tags"]);
    expect(optionDependents(inputs, "category")).toEqual(["topic", "tags"]);
    expect(optionDependents(inputs, "hint")).toEqual(["topic"]);
    expect(optionDependents(inputs, "title")).toEqual([]);
    // A disabled input does not ask.
    const disabled = inputs.map((i) => (i.name === "topic" ? { ...i, disabled: true } : i));
    expect(optionDependents(disabled, "hint")).toEqual([]);
  });

  it("show the latest answer in place of the declared options", () => {
    expect(withOptions(components, {})).toBe(components);
    const shown = inputsOf(withOptions(components, { topic: answered.options, title: [] }));
    expect(shown.find((i) => i.name === "topic")!.options).toEqual(answered.options);
    // Only inputs with `optionsFrom` take an answer.
    expect(shown.find((i) => i.name === "title")!.options).toBeUndefined();
    expect(inputs.find((i) => i.name === "topic")!.options).toEqual([{ value: "apple", label: "Apple" }]);
  });
});

let requests: FormOptionsRequest[] = [];
let answer: (request: FormOptionsRequest) => Promise<Response>;
const byCategory = (request: FormOptionsRequest): SelectOption[] =>
  request.state.category === "veg" ? [{ value: "kale", label: "Kale" }] : answered.options;

const server = setupServer(
  http.get("*/admin/api/auth/me", () => HttpResponse.json({ user: { id: "1", name: "A", email: "a@b.c" }, guard: "admin", permissions: [], superAdmin: true })),
  http.get("*/admin/api/auth/csrf", () => HttpResponse.json({ csrfToken: "t", userId: "1" })),
  http.get("*/admin/api/panel", () =>
    HttpResponse.json({ brand: "Test", user: { id: "1", name: "Ada", email: "a@b.c" }, navigation: [resource.resource] }),
  ),
  http.get("*/admin/api/resources/option_entries/schema", () => HttpResponse.json(schema)),
  http.get("*/admin/api/resources/:slug/fields/:field/options", () => HttpResponse.json({ data: [] })),
  http.post("*/admin/api/resources/option_entries/form/options", async ({ request }) => {
    const body = (await request.json()) as FormOptionsRequest;
    requests.push(body);
    return answer(body);
  }),
);
beforeAll(() => server.listen({ onUnhandledRequest: "error" }));
afterAll(() => server.close());
beforeEach(() => {
  stayOnPurpose();
  requests = [];
  answer = async (request) => HttpResponse.json({ options: byCategory(request) });
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

const topicChoices = () =>
  within(screen.getByLabelText(/^Topic/)).getAllByRole("option").map((o) => o.textContent);
const asked = (input: string) => requests.filter((r) => r.input === input);

describe("the create form", () => {
  it("asks when it opens, then once a dependency's change pauses", async () => {
    mount("/r/option_entries/create");
    await waitFor(() => expect(requests.map((r) => r.input).sort()).toEqual(["tags", "topic"]));
    expect(asked("topic")[0]).toMatchObject({ operation: "create", input: "topic" });
    expect(asked("topic")[0]).not.toHaveProperty("key");
    await waitFor(() => expect(topicChoices()).toContain("Pear"));
    await userEvent.selectOptions(screen.getByLabelText(/^Category/), "Veg");
    await waitFor(() => expect(topicChoices()).toContain("Kale"));
    expect(topicChoices()).not.toContain("Pear");
    expect(asked("topic")[1]!.state).toMatchObject({ category: "veg" });
    // Typing in a dependency asks once, after the pause; an input nothing depends on asks nothing.
    await userEvent.type(screen.getByLabelText(/^Hint/), "abc");
    await waitFor(() => expect(asked("topic")).toHaveLength(3));
    await userEvent.type(screen.getByLabelText(/^Title/), "Plain");
    await delay(600);
    expect(asked("topic")).toHaveLength(3);
    expect(asked("tags")).toHaveLength(2);
  });

  it("applies only the latest answer", async () => {
    mount("/r/option_entries/create");
    await waitFor(() => expect(requests).toHaveLength(2));
    // The answer for veg arrives after the one for fruit.
    answer = async (request) => {
      if (request.state.category === "veg") await delay(700);
      return HttpResponse.json({ options: byCategory(request) });
    };
    const category = screen.getByLabelText(/^Category/);
    await userEvent.selectOptions(category, "Veg");
    await waitFor(() => expect(asked("topic")).toHaveLength(2));
    await userEvent.selectOptions(category, "Fruit");
    await waitFor(() => expect(asked("topic")).toHaveLength(3));
    await delay(900);
    expect(topicChoices()).toContain("Pear");
    expect(topicChoices()).not.toContain("Kale");
  });

  it("keeps the declared options when asking fails", async () => {
    answer = async () => HttpResponse.json(failed);
    mount("/r/option_entries/create");
    await waitFor(() => expect(requests).toHaveLength(2));
    await delay(50);
    expect(topicChoices()).toEqual(["—", "Apple"]);
    expect(screen.queryByRole("alert")).toBeNull();
    // A refused request too.
    answer = async () => HttpResponse.json({ message: "Too many requests.", code: "too_many_requests" }, { status: 429 });
    await userEvent.selectOptions(screen.getByLabelText(/^Category/), "Veg");
    await waitFor(() => expect(asked("topic")).toHaveLength(2));
    await delay(50);
    expect(topicChoices()).toEqual(["—", "Apple"]);
  });
});

it("shows checked values no longer among the options until they are unchecked", async () => {
  const tags = inputs.find((i) => i.name === "tags") as InputSchema;
  const list = [{ ...tags, options: [{ value: "sweet", label: "Sweet" }] }];
  function Harness() {
    const form = useForm<FormValues>({ defaultValues: defaultValues(list, { tags: ["gone", "sweet"] }) });
    return <Components components={list} ctx={{ slug: "option_entries", control: form.control, errors: form.formState.errors }} />;
  }
  render(<Harness />);
  const group = screen.getByRole("group", { name: /Tags/ });
  const gone = within(group).getByRole("checkbox", { name: "gone" });
  expect(gone).toBeChecked();
  expect(within(group).getByRole("checkbox", { name: "Sweet" })).toBeChecked();
  await userEvent.click(gone);
  expect(within(group).queryByRole("checkbox", { name: "gone" })).toBeNull();
});
