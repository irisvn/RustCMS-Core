// v2 views (ADR 0032): an extension's React component, rendered inside the
// panel's own tree (one React, one QueryClient), bound to its mount and the
// session epoch, behind an error boundary.

import { Component, type ComponentType, type ReactNode, useCallback, useEffect, useMemo, useState, useSyncExternalStore } from "react";
import { useNavigate, useRouterState } from "@tanstack/react-router";
import { useSession } from "@/api/context";
import { usePanel } from "@/api/queries";
import { Alert } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import { useLocale, useT } from "@/i18n";
import { type ExtensionApi, type MountToken, extensionApi } from "@/lib/facade";
import { type EventsHandle, type EventsHandlers, type SocketHandle, type SocketHandlers, openEvents, openSocket } from "@/lib/realtime";
import { type ViewKind, translateExtension } from "@/lib/widgets";

/** Where a view is shown. */
export type ViewPlace =
  | { kind: "page"; slug: string; path: string }
  | { kind: "slot"; slot: string; resource: string; recordKey: string }
  | { kind: "cell"; resource: string; field: string };

function deepFreeze<T>(value: T): T {
  if (value && typeof value === "object" && !Object.isFrozen(value)) {
    Object.freeze(value);
    for (const inner of Object.values(value as Record<string, unknown>)) deepFreeze(inner);
  }
  return value;
}

const copy = <T,>(value: T): T => (value === undefined ? value : deepFreeze(structuredClone(value)));

function namespaceOf(place: ViewPlace): string {
  switch (place.kind) {
    case "page":
      return `page:${place.slug}`;
    case "slot":
      return `slot:${place.slot}:${place.resource}:${place.recordKey}`;
    case "cell":
      return `cell:${place.resource}:${place.field}`;
  }
}

interface ViewProps {
  kind: ViewKind;
  name: string;
  component: ComponentType<never>;
  place: ViewPlace;
  data: unknown;
  record?: unknown;
  props?: unknown;
  reload?: () => void;
  fallback?: ReactNode;
}

/** Remounted on every epoch (sign-in, sign-out, rotation) and on Retry. */
export function ReactView(props: ViewProps) {
  const { api } = useSession();
  const subscribe = useCallback((listener: () => void) => api.onEpochChange(listener), [api]);
  const epoch = useSyncExternalStore(subscribe, () => api.epoch);
  const [attempt, setAttempt] = useState(0);
  return (
    <ViewBoundary key={`${epoch}:${attempt}`} kind={props.kind} name={props.name} fallback={props.fallback} onRetry={() => setAttempt((n) => n + 1)}>
      <Instance {...props} epoch={epoch} />
    </ViewBoundary>
  );
}

function Instance({ component, place, data, record, props, reload, epoch }: ViewProps & { epoch: number }) {
  const session = useSession();
  const locale = useLocale();
  const navigate = useNavigate();
  const panel = usePanel();
  const searchStr = useRouterState({ select: (s) => s.location.searchStr });
  // One token per mounted instance: callbacks after unmount or another epoch do nothing.
  const [mount] = useState<MountToken & { handles: Set<EventsHandle | SocketHandle> }>(() => ({
    alive: true,
    epoch,
    handles: new Set(),
  }));
  useEffect(() => {
    mount.alive = true;
    return () => {
      mount.alive = false;
      for (const handle of [...mount.handles]) {
        try {
          handle.close();
        } catch (e) {
          console.error(e);
        }
      }
      mount.handles.clear();
    };
  }, [mount]);
  const live = useCallback(() => mount.alive && session.api.epoch === mount.epoch, [mount, session]);
  const permissions = panel.data && !panel.isPlaceholderData ? panel.data.permissions : undefined;
  const namespace = namespaceOf(place);
  const api: ExtensionApi = useMemo(() => extensionApi(session, mount, namespace), [session, mount, namespace]);
  const guard = useCallback(
    <A extends unknown[]>(f: (...args: A) => void) => (...args: A) => {
      if (live()) f(...args);
    },
    [live],
  );
  // Stable for the instance: hooks keyed on it never reopen their streams.
  const realtime = useMemo(() => {
    // A throwing extension handler is logged; the stream goes on.
    const safe = <H extends object>(handlers: H): H =>
      Object.fromEntries(
        Object.entries(handlers).map(([key, fn]) => [
          key,
          typeof fn === "function"
            ? (...args: unknown[]) => {
                if (!live()) return;
                try {
                  (fn as (...a: unknown[]) => void)(...args);
                } catch (e) {
                  console.error(e);
                }
              }
            : fn,
        ]),
      ) as H;
    const track = <T extends EventsHandle | SocketHandle>(open: () => T): T => {
      const handle = open();
      mount.handles.add(handle);
      const close = () => {
        mount.handles.delete(handle);
        handle.close();
      };
      return { ...handle, close } as T;
    };
    return Object.freeze({
      events: (path: string, handlers: EventsHandlers): EventsHandle => {
        if (!live()) return { close() {} };
        return track(() => openEvents(session.api, path, safe(handlers)));
      },
      socket: (path: string, handlers: SocketHandlers, options?: { reconnect?: boolean }): SocketHandle => {
        if (!live()) return { send: () => false, close() {} };
        return track(() => openSocket(session.api, path, safe(handlers), { reconnect: options?.reconnect === true }));
      },
    });
  }, [live, mount, session]);
  const built = useMemo(() => {
    const common = {
      locale,
      t: translateExtension,
      scope: Object.freeze({ namespace, epoch: mount.epoch }),
      can: (permission: string) => live() && permissions !== undefined && permissions.includes(permission),
    };
    switch (place.kind) {
      case "page":
        return {
          ...common,
          slug: place.slug,
          path: place.path,
          search: new URLSearchParams(searchStr),
          data: copy(data),
          navigate: guard((to: string) => {
            if (typeof to === "string" && to.startsWith("/")) void navigate({ to });
          }),
          reload: guard(() => reload?.()),
          api,
          realtime,
        };
      case "slot":
        return {
          ...common,
          slot: place.slot,
          resource: place.resource,
          recordKey: place.recordKey,
          data: copy(data),
          record: copy(record),
          navigate: guard((to: string) => {
            if (typeof to === "string" && to.startsWith("/")) void navigate({ to });
          }),
          reload: guard(() => reload?.()),
          api,
          realtime,
        };
      case "cell":
        return { ...common, value: copy(data), record: copy(record), props: copy(props) };
    }
  }, [place, data, record, props, reload, locale, namespace, mount, permissions, searchStr, api, live, navigate, guard, realtime]);
  const View = component as ComponentType<typeof built>;
  return <View {...built} />;
}

/** Catches render and effect errors of one instance; Retry remounts it. */
class ViewBoundary extends Component<
  { kind: ViewKind; name: string; fallback?: ReactNode; onRetry: () => void; children: ReactNode },
  { failed: boolean }
> {
  state = { failed: false };

  static getDerivedStateFromError() {
    return { failed: true };
  }

  componentDidCatch(error: unknown) {
    console.error(`rustcms: ${this.props.kind} ${this.props.name} failed`, error);
  }

  render() {
    if (!this.state.failed) return this.props.children;
    if (this.props.fallback !== undefined) return this.props.fallback;
    return <Failed onRetry={this.props.onRetry} />;
  }
}

function Failed({ onRetry }: { onRetry: () => void }) {
  const t = useT();
  return (
    <Alert variant="warning">
      <p>{t("custom.unavailable")}</p>
      <Button type="button" variant="outline" size="sm" className="mt-2" onClick={onRetry}>
        {t("custom.retry")}
      </Button>
    </Alert>
  );
}
