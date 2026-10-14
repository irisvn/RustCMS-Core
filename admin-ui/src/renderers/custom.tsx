// A field edited by a widget from an application extension. The widget owns
// its element; the panel owns the value. Any widget error is contained: the
// widget turns read-only everywhere and its fields are no longer submitted.
import { useEffect, useRef, useSyncExternalStore } from "react";
import type { InputSchema } from "@/api/types";
import { Input } from "@/components/ui/input";
import { type WidgetContext, type WidgetInstance, markFailed, subscribeWidgets, widget } from "@/lib/widgets";
import { hasUnsafeInteger } from "@/schema/form";
import { useLocale } from "@/i18n";
import { translateExtension } from "@/lib/widgets";

const parse = (text: string): unknown => (text === "" ? undefined : JSON.parse(text));

export function CustomField({ id, input, value, onChange, disabled, invalid, describedBy }: {
  id: string;
  input: InputSchema;
  value: string;
  onChange: (value: string) => void;
  disabled?: boolean;
  invalid: boolean;
  describedBy?: string;
}) {
  const element = useRef<HTMLDivElement>(null);
  // The live mount: callbacks of an earlier (unmounted) one are ignored.
  const live = useRef<{ instance: WidgetInstance } | null>(null);
  // What the widget was last told: it is only updated on a change.
  const told = useRef("");
  const locale = useLocale();
  // Everything the widget shows: a new label or language is an update too.
  const state = JSON.stringify([value, !!disabled, invalid, describedBy ?? null, input.label, locale]);
  // Re-rendered when any widget fails, here or in another field.
  const impl = useSyncExternalStore(subscribeWidgets, () => widget(input.widget));
  // The latest props for callbacks fired later by the widget.
  const latest = useRef({ disabled, onChange });
  latest.current = { disabled, onChange };
  const fail = (e: unknown) => markFailed(input.widget ?? "", e);

  const context = (mount: { instance: WidgetInstance } | null): WidgetContext => ({
    id,
    label: input.label,
    // A fresh copy: the widget can never mutate form state in place.
    value: parse(value),
    disabled: !!disabled,
    invalid,
    describedBy,
    props: input.props === undefined ? undefined : structuredClone(input.props),
    locale,
    t: translateExtension,
    onChange: (next) => {
      if (live.current !== mount || !mount || latest.current.disabled) return;
      // Values the wire cannot carry exactly (NaN, Infinity, unsafe integers) are a widget bug.
      if (hasUnsafeInteger(next)) {
        fail(new Error(`widget ${input.widget} reported a number JSON cannot carry exactly`));
        return;
      }
      try {
        latest.current.onChange(next === undefined ? "" : JSON.stringify(next));
      } catch (e) {
        fail(e);
      }
    },
  });

  useEffect(() => {
    const host = element.current;
    if (!impl || !host) return;
    // The mount's identity exists before `mount` runs, so its callbacks can be bound to it.
    const mount = { instance: undefined as unknown as WidgetInstance };
    told.current = state;
    try {
      const instance = impl.mount(host, context(mount));
      if (typeof instance?.update !== "function" || typeof instance?.unmount !== "function") {
        throw new Error(`widget ${input.widget} did not return {update, unmount}`);
      }
      mount.instance = instance;
      live.current = mount;
    } catch (e) {
      host.replaceChildren();
      fail(e);
      return;
    }
    return () => {
      if (live.current === mount) live.current = null;
      try {
        mount.instance.unmount();
      } catch (e) {
        console.error(e);
      }
      host.replaceChildren();
    };
    // Mounted once per widget; changes go through `update` below.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [impl]);

  useEffect(() => {
    const mount = live.current;
    if (!mount || told.current === state) return;
    told.current = state;
    try {
      mount.instance.update(context(mount));
    } catch (e) {
      fail(e);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [state]);

  if (!impl) {
    // Unavailable widget: the stored value as text, never edited or sent.
    return <Input id={id} value={value} disabled readOnly data-fallback="true" aria-describedby={describedBy} />;
  }
  return <div ref={element} className="min-w-0" data-widget={input.widget} />;
}
