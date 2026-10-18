// The `api` of v2 extensions (ADR 0032): the panel's own client under
// `{admin}/api/x`, bound to the mount that made it. Writes go through
// `guardedWrite` (CSRF, one at a time, paused when the outcome is unknown).

import { ApiError, clientError } from "@/api/client";
import { guardedWrite } from "@/api/queries";
import type { Session } from "@/app/session";
import { extensionPath } from "./paths";

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
    const run = () => session.api.request<T>(method, `/x${canonical}${search}`, body);
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

export { ApiError };
