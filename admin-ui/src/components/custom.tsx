// What custom pages, slots and cells draw: server HTML (sanitized by the
// server, inert under the CSP) or an extension script's view.
import { useNavigate } from "@tanstack/react-router";
import { type MouseEvent, useEffect, useRef, useSyncExternalStore } from "react";
import { usePanelConfig } from "@/api/context";
import type { CustomContent } from "@/api/types";
import { Alert } from "@/components/ui/alert";
import { useLocale, useT } from "@/i18n";
import { panelPath } from "@/lib/config";
import {
  type View,
  type ViewContext,
  type ViewInstance,
  type ViewKind,
  markViewFailed,
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
};

/** A script view, or `fallback` while none is registered under `name` (or it failed). */
export function ScriptView(props: ScriptViewProps) {
  const impl = useSyncExternalStore(subscribeWidgets, () => view(props.kind, props.name));
  if (!impl) return <>{props.fallback ?? <Unavailable />}</>;
  return <MountedView {...props} impl={impl} />;
}

function Unavailable() {
  const t = useT();
  return <Alert variant="warning">{t("custom.unavailable")}</Alert>;
}

/** Mounted once, told about changes, unmounted with its element. */
function MountedView({ kind, name, data, record, props, reload, impl }: ScriptViewProps & { impl: View }) {
  const element = useRef<HTMLDivElement>(null);
  const live = useRef<{ instance: ViewInstance } | null>(null);
  const locale = useLocale();
  const open = useOpen();
  const state = JSON.stringify([data ?? null, record ?? null, props ?? null, locale]);
  const latest = useRef({ open, reload });
  latest.current = { open, reload };
  const context = (mount: { instance: ViewInstance } | null): ViewContext => ({
    data: structuredClone(data),
    record: record === undefined ? undefined : structuredClone(record),
    props: props === undefined ? undefined : structuredClone(props),
    locale,
    t: translateExtension,
    // Callbacks of an unmounted view are ignored.
    navigate: (path) => {
      if (live.current === mount) latest.current.open(String(path));
    },
    reload: () => {
      if (live.current === mount) latest.current.reload?.();
    },
  });
  useEffect(() => {
    if (!element.current) return;
    const mount = { instance: undefined as unknown as ViewInstance };
    live.current = mount;
    try {
      mount.instance = impl.mount(element.current, context(mount));
    } catch (e) {
      live.current = null;
      markViewFailed(kind, name, e);
      return;
    }
    return () => {
      live.current = null;
      try {
        mount.instance?.unmount();
      } catch (e) {
        markViewFailed(kind, name, e);
      }
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [impl, kind, name]);
  useEffect(() => {
    const mount = live.current;
    if (!mount?.instance) return;
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
export function CustomContentView({ kind, content, record, reload }: {
  kind: "page" | "slot";
  content: CustomContent;
  record?: unknown;
  reload?: () => void;
}) {
  if (content.kind === "html") return <SafeHtml html={content.html} />;
  return <ScriptView kind={kind} name={content.script} data={content.data} record={record} reload={reload} />;
}
