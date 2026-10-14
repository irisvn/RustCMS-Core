import { act, render, screen } from "@testing-library/react";
import { StrictMode } from "react";
import { useForm } from "react-hook-form";
import type { InputSchema } from "@/api/types";
import { type WidgetContext, loadExtensions, registerWidget, resetWidgets, widget } from "@/lib/widgets";
import { Components } from "@/renderers/form";
import { type FormValues, defaultValues, payloadOf, zodSchema } from "@/schema/form";

const color = (extra: Partial<InputSchema> = {}): InputSchema => ({
  name: "accent",
  label: "Accent",
  type: "custom",
  widget: "acme/color",
  valueType: "string",
  nullable: true,
  rules: [],
  props: { palette: ["#ff0000"] },
  ...extra,
} as InputSchema);

/** A widget recording its lifecycle, with access to the latest context (and each mount's). */
function recorder(name = "acme/color") {
  const log: string[] = [];
  const mounts: WidgetContext[] = [];
  let ctx: WidgetContext | undefined;
  registerWidget(name, {
    apiVersion: 1,
    mount: (el, c) => {
      log.push(`mount ${JSON.stringify(c.value)}`);
      mounts.push(c);
      ctx = c;
      el.textContent = "picker";
      return {
        update: (next) => {
          log.push(`update ${JSON.stringify(next.value)} ${next.disabled}`);
          ctx = next;
        },
        unmount: () => log.push("unmount"),
      };
    },
  });
  return { log, mounts, ctx: () => ctx! };
}

let values: FormValues = {};
function Harness({ input, inputs, data, readOnly }: { input?: InputSchema; inputs?: InputSchema[]; data?: Record<string, unknown>; readOnly?: boolean }) {
  const all = inputs ?? [input!];
  const form = useForm<FormValues>({ defaultValues: defaultValues(all, data) });
  values = form.watch();
  return <Components components={all} ctx={{ slug: "gadgets", control: form.control, errors: {}, readOnly }} />;
}

afterEach(() => resetWidgets());

describe("extension loading", () => {
  it("moves on after a script that does not load in time", async () => {
    vi.useFakeTimers();
    const error = vi.spyOn(console, "error").mockImplementation(() => {});
    try {
      const loading = loadExtensions("/admin", { scripts: ["assets/ext/a.00000000.js", "assets/ext/b.00000000.js"], styles: ["assets/ext/a.00000000.css"] });
      const scripts = () => [...document.head.querySelectorAll("script")].map((s) => s.getAttribute("src"));
      expect(scripts()).toEqual(["/admin/assets/ext/a.00000000.js"]);
      expect(document.head.querySelector("link")?.getAttribute("href")).toBe("/admin/assets/ext/a.00000000.css");
      await vi.advanceTimersByTimeAsync(10_000);
      expect(scripts()).toEqual(["/admin/assets/ext/a.00000000.js", "/admin/assets/ext/b.00000000.js"]);
      document.head.querySelectorAll("script")[1]!.dispatchEvent(new Event("load"));
      await loading;
      expect(error).toHaveBeenCalledWith(expect.stringMatching(/a\.00000000\.js did not load in time/));
    } finally {
      document.head.replaceChildren();
      error.mockRestore();
      vi.useRealTimers();
    }
  });
});

describe("widget registry", () => {
  it("refuses bad names, duplicates, other API versions and late registrations", async () => {
    const ok = { apiVersion: 1 as const, mount: () => ({ update() {}, unmount() {} }) };
    expect(() => registerWidget("Color", ok)).toThrow(/invalid widget name/);
    expect(() => registerWidget("acme/color", { ...ok, apiVersion: 2 } as never)).toThrow(/API version/);
    registerWidget("acme/color", ok);
    expect(() => registerWidget("acme/color", ok)).toThrow(/already registered/);
    await loadExtensions("/admin", { scripts: [], styles: [] });
    expect((window as unknown as { rustcms: { apiVersion: number } }).rustcms.apiVersion).toBe(1);
    expect(() => registerWidget("acme/other", ok)).toThrow(/while extensions load/);
  });
});

describe("custom inputs", () => {
  it("mounts, updates and unmounts the widget; only onChange changes the value", () => {
    const { log, ctx } = recorder();
    const { unmount } = render(<Harness input={color()} data={{ accent: "#00ff00" }} />);
    expect(screen.getByText("picker")).toBeInTheDocument();
    expect(log).toEqual(['mount "#00ff00"']);
    expect(ctx().props).toEqual({ palette: ["#ff0000"] });
    act(() => ctx().onChange("#ff0000"));
    expect(values.accent).toBe('"#ff0000"');
    expect(log.at(-1)).toBe('update "#ff0000" false');
    act(() => ctx().onChange(null));
    expect(payloadOf([color()], values)).toEqual({ accent: null });
    act(() => ctx().onChange(undefined));
    expect(payloadOf([color()], values)).toEqual({});
    unmount();
    expect(log.at(-1)).toBe("unmount");
  });

  it("survives StrictMode and ignores callbacks after unmount", () => {
    const { log, mounts, ctx } = recorder();
    const { unmount } = render(<StrictMode><Harness input={color()} data={{ accent: "#00ff00" }} /></StrictMode>);
    expect(log.slice(0, 3)).toEqual(['mount "#00ff00"', "unmount", 'mount "#00ff00"']);
    // The discarded first mount cannot change the form.
    act(() => mounts[0]!.onChange("#999999"));
    expect(values.accent).toBe('"#00ff00"');
    const late = ctx();
    unmount();
    act(() => late.onChange("#123456"));
    expect(values.accent).toBe('"#00ff00"');
  });

  it("is read-only in view mode, and the widget is told", () => {
    const { log, ctx } = recorder();
    render(<Harness input={color()} data={{ accent: "#00ff00" }} readOnly />);
    expect(ctx().disabled).toBe(true);
    act(() => ctx().onChange("#ff0000"));
    expect(values.accent).toBe('"#00ff00"');
    expect(log).toEqual(['mount "#00ff00"']);
  });

  it("shows an unregistered widget read-only and never submits it", () => {
    const { container } = render(<Harness input={color()} data={{ accent: "#00ff00" }} />);
    const fallback = container.querySelector("[data-fallback]") as HTMLInputElement;
    expect(fallback.disabled).toBe(true);
    expect(fallback.value).toBe('"#00ff00"');
    expect(payloadOf([color()], values)).toEqual({});
  });

  it("isolates a failing widget: read-only from then on, not submitted", () => {
    const error = vi.spyOn(console, "error").mockImplementation(() => {});
    registerWidget("acme/color", { apiVersion: 1, mount: () => { throw new Error("boom"); } });
    const { container } = render(<Harness input={color()} data={{ accent: "#00ff00" }} />);
    expect(container.querySelector("[data-fallback]")).not.toBeNull();
    expect(widget("acme/color")).toBeUndefined();
    expect(payloadOf([color()], values)).toEqual({});
    error.mockRestore();
  });

  it("turns every field of a failing widget read-only, not only the one that failed", () => {
    const error = vi.spyOn(console, "error").mockImplementation(() => {});
    const { mounts } = recorder();
    const other = color({ name: "border", label: "Border" });
    const { container } = render(<Harness inputs={[color(), other]} data={{ accent: "#00ff00", border: "#000000" }} />);
    expect(mounts).toHaveLength(2);
    // A value the wire cannot carry exactly is a widget bug: the widget is disabled.
    act(() => mounts[0]!.onChange(Number.POSITIVE_INFINITY));
    expect(container.querySelectorAll("[data-fallback]")).toHaveLength(2);
    act(() => mounts[1]!.onChange("#111111"));
    expect(values).toEqual({ accent: '"#00ff00"', border: '"#000000"' });
    expect(payloadOf([color(), other], values)).toEqual({});
    error.mockRestore();
  });

  it("refuses unsafe integers and widgets that return no instance", () => {
    const error = vi.spyOn(console, "error").mockImplementation(() => {});
    const { ctx } = recorder();
    render(<Harness input={color({ valueType: "json" } as Partial<InputSchema>)} data={{ accent: { n: 1 } }} />);
    act(() => ctx().onChange({ n: 2 ** 53 + 2 }));
    expect(values.accent).toBe('{"n":1}');
    expect(widget("acme/color")).toBeUndefined();

    registerWidget("acme/empty", { apiVersion: 1, mount: () => ({}) as never });
    const { container } = render(<Harness input={color({ name: "x", widget: "acme/empty" })} />);
    expect(container.querySelector("[data-fallback]")).not.toBeNull();
    error.mockRestore();
  });

  it("blocks a required field whose widget is unavailable, with a clear message", () => {
    // Even with a value (a default): it would not be sent.
    const kept = zodSchema([color({ rules: [{ rule: "required" }] } as Partial<InputSchema>)]).safeParse({ accent: '"#fff"' });
    expect(kept.error?.issues[0]?.message).toMatch(/widget is not available/);
    const required = color({ rules: [{ rule: "required" }] } as Partial<InputSchema>);
    const missing = zodSchema([required]).safeParse({ accent: "" });
    expect(missing.error?.issues[0]?.message).toMatch(/widget is not available/);
    recorder();
    expect(zodSchema([required]).safeParse({ accent: "" }).error?.issues[0]?.message).toBe("is required");
    expect(zodSchema([required]).safeParse({ accent: '"#fff"' }).success).toBe(true);
  });
});
