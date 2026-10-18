// What custom pages, slots and cells draw: server HTML (sanitized by the
// server, inert under the CSP) or an extension script's view.
import { useNavigate } from "@tanstack/react-router";
import { type MouseEvent, useEffect, useRef, useSyncExternalStore } from "react";
import { useApi, usePanelConfig } from "@/api/context";
import type { CustomContent } from "@/api/types";
import { Alert } from "@/components/ui/alert";
import { useLocale, useT } from "@/i18n";
import { panelPath } from "@/lib/config";
import { type EventsHandle, openEvents, openSocket } from "@/lib/realtime";
import { ReactView, type ViewPlace } from "./react-view";
import {
  type View,
  type ViewContext,
  type ViewInstance,
  type ViewKind,
  markViewFailed,
  reactView,
  subscribeWidgets,
  translateExtension,
  view,
} from "@/lib/widgets";

/** Navigation of the panel, for views: only paths inside it. */
function useOpen() {
  const navigate = useNavigate();
  const { basePath } = usePanelConfig();
  return (href: string) => {
    const path = panelPath(href.startsWith("/") && !href.startsWith(basePath) ? `${basePath}${href}` : href, basePath);
    if (path !== null) void navigate({ to: path });
  };
}

/** HTML the server rendered and cleaned; links inside the panel navigate without a reload. */
export function SafeHtml({ html }: { html: string }) {
  const navigate = useNavigate();
  const { basePath } = usePanelConfig();
  const click = (event: MouseEvent<HTMLDivElement>) => {
    if (event.defaultPrevented || event.button !== 0 || event.metaKey || event.ctrlKey || event.shiftKey || event.altKey) return;
    const link = (event.target as Element).closest("a");
    if (!link || link.hasAttribute("target") || link.hasAttribute("download")) return;
    const path = panelPath(link.getAttribute("href") ?? "", basePath);
    if (path === null) return;
    event.preventDefault();
    void navigate({ to: path });
  };
  // The only raw HTML of the panel: the application's own template, escaped
  // and cleaned by the server's allowlist (no script, style, handler, form,
  // frame, id or foreign URL), and inert under the CSP anyway.
  // eslint-disable-next-line no-restricted-syntax
  return <div className="custom-html min-w-0" onClick={click} dangerouslySetInnerHTML={{ __html: html }} />;
}

type ScriptViewProps = {
  kind: ViewKind;
  name: string;
  data: unknown;
  record?: unknown;
  props?: unknown;
  reload?: () => void;
  fallback?: React.ReactNode;
  /** Where it is shown (v2 views' props). */
  place?: ViewPlace;
};

/** A mounted view and the streams it opened (closed with it). */
interface Mount {
  instance: ViewInstance;
  /** `mounting` while its `mount` runs; once `disposed`, nothing opens again. */
  phase: "mounting" | "active" | "disposed";
  streams: Set<EventsHandle>;
}

function closeStreams(mount: Mount) {
  mount.phase = "disposed";
  for (const stream of mount.streams) stream.close();
  mount.streams.clear();
}

/** A script view, or `fallback` while none is registered under `name` (or it failed). */
export function ScriptView(props: ScriptViewProps) {
  const impl = useSyncExternalStore(subscribeWidgets, () => view(props.kind, props.name));
  const react = useSyncExternalStore(subscribeWidgets, () => reactView(props.kind, props.name));
  if (react) {
    const place = props.place ?? { kind: "cell" as const, resource: "", field: props.name };
    return <ReactView {...props} component={react} place={place} />;
  }
  if (!impl) return <>{props.fallback ?? <Unavailable />}</>;
  return <MountedView {...props} impl={impl} />;
}

function Unavailable() {
  const t = useT();
  return <Alert variant="warning">{t("custom.unavailable")}</Alert>;
}

/**
 * Mounted once per session epoch, told about changes, unmounted with its
 * element. Callbacks of an unmounted view, or from an earlier epoch, are ignored.
 */
function MountedView({ kind, name, data, record, props, reload, impl }: ScriptViewProps & { impl: View }) {
  const element = useRef<HTMLDivElement>(null);
  const live = useRef<Mount | null>(null);
  const api = useApi();
  const epoch = api.epoch;
  const locale = useLocale();
  const open = useOpen();
  const state = JSON.stringify([data ?? null, record ?? null, props ?? null, locale]);
  const latest = useRef({ open, reload });
  latest.current = { open, reload };
  const current = (mount: Mount) => live.current === mount && api.epoch === epoch;
  const context = (mount: Mount): ViewContext => ({
    data: structuredClone(data),
    record: record === undefined ? undefined : structuredClone(record),
    props: props === undefined ? undefined : structuredClone(props),
    locale,
    t: translateExtension,
    navigate: (path) => {
      if (current(mount)) latest.current.open(String(path));
    },
    reload: () => {
      if (current(mount)) latest.current.reload?.();
    },
    realtime: {
      events: (path, handlers) => {
        // Opened by a view no longer shown (or of an earlier session): nothing.
        const allowed = mount.phase === "mounting" || (mount.phase === "active" && current(mount));
        if (!allowed) return { close() {} };
        const handle = openEvents(api, String(path), handlers);
        mount.streams.add(handle);
        return {
          close: () => {
            mount.streams.delete(handle);
            handle.close();
          },
        };
      },
      socket: (path, handlers, options) => {
        const allowed = mount.phase === "mounting" || (mount.phase === "active" && current(mount));
        if (!allowed) return { send: () => false, close() {} };
        const reconnect = options?.reconnect === true;
        const handle = openSocket(api, String(path), handlers, { reconnect });
        mount.streams.add(handle);
        return {
          send: (data) => handle.send(data),
          close: () => {
            mount.streams.delete(handle);
            handle.close();
          },
        };
      },
    },
  });
  useEffect(() => {
    const host = element.current;
    if (!host) return;
    // The mount's identity exists before `mount` runs, so its callbacks can be bound to it.
    const mount: Mount = { instance: undefined as unknown as ViewInstance, phase: "mounting", streams: new Set() };
    try {
      const instance = impl.mount(host, context(mount));
      if (typeof instance?.update !== "function" || typeof instance?.unmount !== "function") {
        throw new Error(`view ${name} did not return {update, unmount}`);
      }
      mount.instance = instance;
      mount.phase = "active";
      live.current = mount;
    } catch (e) {
      closeStreams(mount);
      host.replaceChildren();
      markViewFailed(kind, name, e);
      return;
    }
    return () => {
      if (live.current === mount) live.current = null;
      closeStreams(mount);
      try {
        mount.instance.unmount();
      } catch (e) {
        console.error(e);
      }
      host.replaceChildren();
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [impl, kind, name, epoch]);
  useEffect(() => {
    const mount = live.current;
    if (!mount) return;
    try {
      mount.instance.update(context(mount));
    } catch (e) {
      markViewFailed(kind, name, e);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [state]);
  return <div ref={element} className="min-w-0" />;
}

/** A page's or slot's content. */
export function CustomContentView({ kind, content, record, reload, place }: {
  kind: "page" | "slot";
  content: CustomContent;
  record?: unknown;
  reload?: () => void;
  place?: ViewPlace;
}) {
  if (content.kind === "html") return <SafeHtml html={content.html} />;
  return <ScriptView kind={kind} name={content.script} data={content.data} record={record} reload={reload} place={place} />;
}
