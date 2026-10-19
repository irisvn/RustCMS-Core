// The `api` of v2 extensions (ADR 0032): the panel's own client under
// `{admin}/api/x`, bound to the mount that made it. Writes go through
// `guardedWrite` (CSRF, one at a time, paused when the outcome is unknown).

import type { QueryClient } from "@tanstack/react-query";
import { ApiError, clientError } from "@/api/client";
import { guardedWrite, invalidateAfterWrite, keyedWrite } from "@/api/queries";
import type { Session } from "@/app/session";
import { extensionPath, recordKey, resourceName, resourcePath } from "./paths";

/** The mount an extension's callbacks belong to. */
export interface MountToken {
  alive: boolean;
  epoch: number;
}

export type QueryValue = string | number | boolean | Array<string | number | boolean>;
export type Query = Record<string, QueryValue | undefined>;

/** `query` as sorted `[name, value]` pairs (duplicates kept, in order): both the cache key part and what is sent. */
export function queryPairs(query: Query | undefined): Array<[string, string]> {
  const pairs: Array<[string, string]> = [];
  for (const name of Object.keys(query ?? {}).sort()) {
    const value = query![name];
    if (value === undefined) continue;
    for (const item of Array.isArray(value) ? value : [value]) pairs.push([name, String(item)]);
  }
  return pairs;
}

export interface ExtensionApi {
  get<T>(path: string, options?: { query?: Query }): Promise<T>;
  post<T>(path: string, body?: unknown, options?: { query?: Query }): Promise<T>;
  put<T>(path: string, body?: unknown, options?: { query?: Query }): Promise<T>;
  patch<T>(path: string, body?: unknown, options?: { query?: Query }): Promise<T>;
  delete<T>(path: string, options?: { query?: Query }): Promise<T>;
}

/** The error of a call made, or answered, after its mount went stale. */
export const sessionChanged = () => clientError(0, "session_changed", "error.sessionChanged");

const live = (session: Session, mount: MountToken) => mount.alive && session.api.epoch === mount.epoch;

export function extensionApi(session: Session, mount: MountToken, namespace: string): ExtensionApi {
  const call = async <T>(method: string, path: string, body: unknown, query: Query | undefined): Promise<T> => {
    if (!live(session, mount)) throw sessionChanged();
    // Throws on a path outside the extension rules (nothing is sent).
    const canonical = extensionPath(path);
    const pairs = queryPairs(query);
    const search = pairs.length > 0 ? `?${new URLSearchParams(pairs).toString()}` : "";
    // Rechecked when it runs: a queued write of a mount that went stale is never sent.
    const run = () =>
      live(session, mount) ? session.api.request<T>(method, `/x${canonical}${search}`, body) : Promise.reject(sessionChanged());
    let result: T;
    try {
      // A maybe-applied write is recorded by `guardedWrite` before this sees it.
      result = method === "GET" ? await run() : await guardedWrite(session, `x:${namespace}`, run);
    } catch (e) {
      if (!live(session, mount)) throw sessionChanged();
      throw e;
    }
    if (!live(session, mount)) throw sessionChanged();
    return result;
  };
  return Object.freeze({
    get: <T,>(path: string, options?: { query?: Query }) => call<T>("GET", path, undefined, options?.query),
    post: <T,>(path: string, body?: unknown, options?: { query?: Query }) => call<T>("POST", path, body, options?.query),
    put: <T,>(path: string, body?: unknown, options?: { query?: Query }) => call<T>("PUT", path, body, options?.query),
    patch: <T,>(path: string, body?: unknown, options?: { query?: Query }) => call<T>("PATCH", path, body, options?.query),
    delete: <T,>(path: string, options?: { query?: Query }) => call<T>("DELETE", path, undefined, options?.query),
  });
}

/** `props.resources` (ADR 0032, ui 2): the resource API, as the panel's own pages call it. */
export interface ExtensionResources {
  /** Marks `slug` as used by this view: its paused-write banner shows here. */
  track(slug: string): void;
  list<T>(slug: string, query?: Query): Promise<T>;
  get<T>(slug: string, key: string): Promise<T>;
  create<T>(slug: string, data: Record<string, unknown>): Promise<T>;
  update<T>(slug: string, key: string, data: Record<string, unknown>): Promise<T>;
  delete(slug: string, key: string): Promise<void>;
  restore<T>(slug: string, key: string): Promise<T>;
  forceDelete(slug: string, key: string): Promise<void>;
  action<T>(slug: string, name: string, keys: string[], data?: Record<string, unknown>): Promise<T>;
}

type Envelope<T> = { data: T };

/**
 * Bound to `mount` like `extensionApi`. Writes share the resource's own
 * paused-write scope (its slug) with the built-in pages; creates and actions
 * carry an `Idempotency-Key`. A pause is recorded, and a success invalidates,
 * before a stale mount's delivery is refused.
 */
export function extensionResources(session: Session, mount: MountToken, client: QueryClient, used: (slug: string) => void): ExtensionResources {
  const alive = () => live(session, mount);
  const gate = <T,>(send: () => Promise<T>) => (alive() ? send() : Promise.reject(sessionChanged()));
  const path = (segments: string[]) => resourcePath(session.api.apiPath, segments);
  const settle = async <T,>(slug: string, write: boolean, run: () => Promise<T>): Promise<T> => {
    if (!alive()) throw sessionChanged();
    used(slug);
    let result: T;
    try {
      result = await run();
    } catch (e) {
      if (!alive()) throw sessionChanged();
      throw e;
    }
    if (write) await invalidateAfterWrite(client, slug);
    if (!alive()) throw sessionChanged();
    return result;
  };
  const read = <T,>(slug: string, segments: string[], query?: Query) => {
    const pairs = queryPairs(query);
    const search = pairs.length > 0 ? `?${new URLSearchParams(pairs).toString()}` : "";
    const url = `${path(segments)}${search}`;
    return settle(slug, false, () => gate(() => session.api.request<T>("GET", url, undefined)));
  };
  const guarded = <T,>(slug: string, method: string, segments: string[], body?: unknown) => {
    const url = path(segments);
    return settle(slug, true, () => guardedWrite(session, slug, () => gate(() => session.api.request<T>(method, url, body))));
  };
  const keyed = <T,>(slug: string, segments: string[], body: unknown) => {
    const url = path(segments);
    return settle(slug, true, () => keyedWrite<T>(session, slug, "POST", url, body, undefined, gate));
  };
  // Invalid names and keys reject (nothing is sent), as every other failure does.
  const rejecting =
    <A extends unknown[], R>(f: (...args: A) => Promise<R>) =>
    (...args: A): Promise<R> => {
      try {
        return f(...args);
      } catch (e) {
        return Promise.reject(e);
      }
    };
  return Object.freeze({
    track: (slug: string) => {
      if (alive()) used(resourceName(slug));
    },
    list: rejecting(<T,>(slug: string, query?: Query) => read<T>(resourceName(slug), [slug, "records"], query)),
    get: rejecting(<T,>(slug: string, key: string) =>
      read<Envelope<T>>(resourceName(slug), [slug, "records", recordKey(key)]).then((r) => r.data),
    ),
    create: rejecting(<T,>(slug: string, data: Record<string, unknown>) =>
      keyed<Envelope<T>>(resourceName(slug), [slug, "records"], data).then((r) => r.data),
    ),
    update: rejecting(<T,>(slug: string, key: string, data: Record<string, unknown>) =>
      guarded<Envelope<T>>(resourceName(slug), "PATCH", [slug, "records", recordKey(key)], data).then((r) => r.data),
    ),
    delete: rejecting((slug: string, key: string) => guarded<void>(resourceName(slug), "DELETE", [slug, "records", recordKey(key)])),
    restore: rejecting(<T,>(slug: string, key: string) =>
      guarded<Envelope<T>>(resourceName(slug), "POST", [slug, "records", recordKey(key), "restore"], {}).then((r) => r.data),
    ),
    forceDelete: rejecting((slug: string, key: string) =>
      guarded<void>(resourceName(slug), "DELETE", [slug, "records", recordKey(key), "force"]),
    ),
    action: rejecting(<T,>(slug: string, name: string, keys: string[], data?: Record<string, unknown>) => {
      if (!Array.isArray(keys)) throw new Error("action keys must be an array");
      return keyed<T>(resourceName(slug), [slug, "actions", resourceName(name, "action")], { keys: keys.map(recordKey), data: data ?? {} });
    }),
  }) as ExtensionResources;
}

export { ApiError };
