// HTTP client of the admin API: same-origin cookies, CSRF header on writes,
// typed errors. Writes are never retried automatically. Messages are shown in
// the panel's language: the server's are translated by code (`issues`), with
// its English text as the fallback.

import { getLocale, has, issueText, t, type Key, type Params } from "@/i18n";

export interface Issue {
  code: string;
  params?: Record<string, string>;
}

export interface ErrorBody {
  message: string;
  code: string;
  /** Which conflict (`write_conflict`): `busy`, `upload_expired`, `receipt_expired`. */
  reason?: string;
  errors?: Record<string, string[]>;
  formErrors?: string[];
  /** Stable code and parameters of each message in `errors`, same order. */
  issues?: Record<string, Issue[]>;
  formIssues?: Issue[];
  /** Set by the client: a write whose outcome is unknown (never inferred from text). */
  outcomeUnknown?: boolean;
  /** Set by the client from `Retry-After` (seconds). */
  retryAfter?: number;
  /** Set by the client: its own message, translated whenever it is shown. */
  messageKey?: Key;
  messageParams?: Params;
}

export class ApiError extends Error {
  readonly status: number;
  readonly body: ErrorBody;

  constructor(status: number, body: ErrorBody) {
    super(body.message);
    // Read in the panel's current language, also after it changes.
    Object.defineProperty(this, "message", { get: () => localized(this.body), configurable: true });
    this.status = status;
    this.body = body;
  }

  get code() {
    return this.body.code;
  }

  /** Field messages in the panel's language. */
  get fieldErrors(): Record<string, string[]> {
    const out: Record<string, string[]> = {};
    for (const [field, messages] of Object.entries(this.body.errors ?? {})) {
      out[field] = messages.map((m, i) => issueText(this.body.issues?.[field]?.[i], m));
    }
    return out;
  }

  /** Form messages in the panel's language. */
  get formErrors(): string[] {
    return (this.body.formErrors ?? []).map((m, i) => issueText(this.body.formIssues?.[i], m));
  }

  /** Whether the server reported `code` for any field or for the form (e.g. `stale`). */
  hasIssue(code: string): boolean {
    return (this.body.formIssues ?? []).some((i) => i.code === code) ||
      Object.values(this.body.issues ?? {}).some((list) => list.some((i) => i.code === code));
  }

  /** A write whose outcome is unknown: resubmitting could apply it twice. */
  get maybeApplied(): boolean {
    return ["outcome_unknown", "incomplete_response"].includes(this.code) || this.body.outcomeUnknown === true;
  }
}

const SAFE = new Set(["GET", "HEAD"]);

export interface ClientOptions {
  apiPath: string;
  /** Called on any 401: the session is gone. */
  onUnauthenticated?: () => void;
  /** Called with `/auth/me` after a CSRF failure: the session may now belong to someone else. */
  onSessionProbe?: (me: unknown) => void;
  /**
   * Called with the user a fresh CSRF token belongs to (null: signed out).
   * Returning false aborts the pending write: the session changed accounts.
   */
  verifyIdentity?: (userId: string | null) => boolean;
  /**
   * Called with the user every admin response was produced for. Returning false
   * discards the response: the page belongs to another account.
   */
  observeIdentity?: (userId: string) => boolean;
  fetch?: typeof fetch;
}

export class ApiClient {
  private csrf: string | null = null;
  /** Bumped whenever session state is dropped: work started before is abandoned. */
  private generation = 0;
  private csrfLoading: Promise<string> | null = null;
  private readonly opts: ClientOptions;

  constructor(opts: ClientOptions) {
    this.opts = opts;
  }

  get apiPath() {
    return this.opts.apiPath;
  }

  /** Session epoch: changes on sign-in, sign-out, 401, CSRF failures and account changes. */
  get epoch() {
    return this.generation;
  }

  /** Forgets the CSRF token (after login/logout the server rotates it). */
  resetCsrf() {
    this.generation += 1;
    this.csrf = null;
    this.csrfLoading = null;
  }

  /**
   * Fetches a new CSRF token on the next write without starting a new epoch
   * (work in flight stays valid): for re-sending a write after a CSRF refusal.
   */
  refreshCsrf() {
    this.csrf = null;
    this.csrfLoading = null;
  }

  /** The session's CSRF token, fetched once and kept in memory only. */
  async csrfToken(): Promise<string> {
    if (this.csrf) return this.csrf;
    const generation = this.generation;
    this.csrfLoading ??= this.request<{ csrfToken: string; userId?: string | null }>("GET", "/auth/csrf").then((r) => {
      // The session was reset while the token was on its way: it may belong to someone else.
      if (generation !== this.generation) throw sessionReset();
      // A token is only used for the account this page was built for.
      if (this.opts.verifyIdentity && !this.opts.verifyIdentity(r.userId ?? null)) {
        throw clientError(409, "session_changed", "error.otherAccount");
      }
      this.csrf = r.csrfToken;
      return r.csrfToken;
    });
    try {
      return await this.csrfLoading;
    } finally {
      this.csrfLoading = null;
    }
  }

  get<T>(path: string, query?: URLSearchParams | Record<string, string | undefined>) {
    return this.request<T>("GET", path + queryString(query));
  }

  post<T>(path: string, body?: unknown) {
    return this.request<T>("POST", path, body);
  }

  patch<T>(path: string, body?: unknown) {
    return this.request<T>("PATCH", path, body);
  }

  delete<T>(path: string) {
    return this.request<T>("DELETE", path);
  }

  /**
   * A file the server sends (`GET`): its bytes, or the server's error as an
   * `ApiError` (a JSON body) like any request.
   */
  async download(path: string): Promise<Blob> {
    const generation = this.generation;
    const doFetch = this.opts.fetch ?? fetch;
    let response: Response;
    try {
      response = await doFetch(this.opts.apiPath + path, {
        method: "GET",
        headers: { "Accept-Language": getLocale(), "X-Rustcms-Locale": getLocale() },
        credentials: "same-origin",
        // A request's own policy: on the token pages (`Referrer-Policy: no-referrer`)
        // a same-origin POST would otherwise carry `Origin: null` and be refused.
        referrerPolicy: "same-origin",
        cache: "no-store",
      });
    } catch {
      throw clientError(0, "network_error", "error.network");
    }
    if (generation !== this.generation) throw sessionReset();
    const owner = response.headers.get("X-Rustcms-User");
    if (owner !== null && this.opts.observeIdentity && !this.opts.observeIdentity(owner)) throw sessionReset();
    if (!response.ok) {
      const payload = await response.json().catch(() => null);
      if (response.status === 401) this.opts.onUnauthenticated?.();
      throw new ApiError(response.status, errorBody(response, payload));
    }
    const blob = await response.blob();
    // Signed out or another account while the bytes arrived: not this session's file.
    if (generation !== this.generation) throw sessionReset();
    return blob;
  }

  async request<T>(method: string, path: string, body?: unknown, extra?: Record<string, string>): Promise<T> {
    // Labels in the answer are in the language this tab shows (the cache is keyed by it).
    const headers: Record<string, string> = {
      ...extra,
      Accept: "application/json",
      "Accept-Language": getLocale(),
      "X-Rustcms-Locale": getLocale(),
    };
    // Multipart bodies (uploads) get their boundary from the browser; files are sent as they are.
    const multipart = typeof FormData !== "undefined" && body instanceof FormData;
    const file = typeof Blob !== "undefined" && body instanceof Blob;
    if (file) headers["Content-Type"] = (body as Blob).type || "application/octet-stream";
    else if (body !== undefined && !multipart) headers["Content-Type"] = "application/json";
    const generation = this.generation;
    // An answer to a request from an earlier session state (before a sign-in,
    // sign-out or account change) never reaches identity or auth handling: it
    // could otherwise clear or redirect the current session.
    const obsolete = () => {
      if (generation === this.generation) return null;
      return SAFE.has(method)
        ? sessionReset()
        : clientError(409, "outcome_unknown", "error.sessionChangedWrite");
    };
    if (!SAFE.has(method)) {
      headers["X-CSRF-Token"] = await this.csrfToken();
      // A write prepared before a sign-out, 401 or account change is never sent.
      if (generation !== this.generation) throw sessionReset();
    }
    const doFetch = this.opts.fetch ?? fetch;
    let response: Response;
    try {
      response = await doFetch(this.opts.apiPath + path, {
        method,
        headers,
        body: body === undefined ? undefined : multipart ? (body as FormData) : file ? (body as Blob) : JSON.stringify(body),
        credentials: "same-origin",
        // A request's own policy: on the token pages (`Referrer-Policy: no-referrer`)
        // a same-origin POST would otherwise carry `Origin: null` and be refused.
        referrerPolicy: "same-origin",
        cache: "no-store",
      });
    } catch {
      // A write may have reached the server before the connection failed: never claim it did not.
      const write = !SAFE.has(method);
      throw clientError(0, "network_error", write ? "error.networkWrite" : "error.network", { outcomeUnknown: write });
    }
    const stale = obsolete();
    if (stale) throw stale;
    const owner = response.headers.get("X-Rustcms-User");
    if (owner !== null && this.opts.observeIdentity && !this.opts.observeIdentity(owner)) {
      throw sessionReset();
    }
    if (response.status === 204) return undefined as T;
    let payload: unknown = null;
    let unreadable = false;
    try {
      payload = await response.json();
    } catch {
      unreadable = true;
    }
    const staleAfterBody = obsolete();
    if (staleAfterBody) throw staleAfterBody;
    if (response.ok && (unreadable || payload === null)) {
      // Headers said success but the body never arrived intact: a write may well be committed.
      // Only a sent write can have been applied; a cut-off read is just a failed read.
      throw SAFE.has(method)
        ? clientError(response.status, "incomplete_read", "error.incompleteRead")
        : clientError(response.status, "incomplete_response", "error.incompleteWrite");
    }
    if (!response.ok && !SAFE.has(method) && (response.status >= 500 || response.status === 408)) {
      // A server error, timeout or gateway error says nothing about whether the
      // write committed (e.g. the connection dropped while acknowledging COMMIT).
      throw clientError(response.status, "outcome_unknown", "error.unconfirmed");
    }
    if (!response.ok) {
      const error = new ApiError(response.status, errorBody(response, payload));
      if (response.status === 401) {
        // The session decides (it holds auth handling during a credential rotation);
        // without one, just forget the token.
        if (this.opts.onUnauthenticated) this.opts.onUnauthenticated();
        else this.resetCsrf();
      } else if (error.code === "csrf_mismatch") {
        // Stale token (e.g. the session expired or rotated): drop it, and find out
        // whether the session is still there. The write is not replayed.
        this.resetCsrf();
        const me = await this.request("GET", "/auth/me").catch(() => undefined);
        if (me !== undefined) this.opts.onSessionProbe?.(me);
        error.body.messageKey = "error.csrf";
      }
      throw error;
    }
    return payload as T;
  }
}

export const sessionReset = () => clientError(409, "session_changed", "error.sessionChanged");

/** An error the client raises itself: its message is a catalog key. */
export function clientError(status: number, code: string, key: Key, extra: Partial<ErrorBody> = {}): ApiError {
  return new ApiError(status, { code, message: t(key), messageKey: key, ...extra });
}

const FALLBACK: Record<number, string> = {
  400: "bad_request",
  404: "not_found",
  405: "method_not_allowed",
  408: "timeout",
  413: "payload_too_large",
  429: "too_many_requests",
};

/** The top-level message in the panel's language: by code (and reason), else the server's text. */
function localized(body: ErrorBody): string {
  if (body.messageKey) return t(body.messageKey, body.messageParams);
  if (body.code === "too_many_requests" && body.retryAfter) return t("error.too_many_requests_retry", { seconds: body.retryAfter });
  const specific = body.reason ? `error.${body.code}.${body.reason}` : null;
  const key = specific && has(specific) ? specific : `error.${body.code}`;
  return has(key) ? t(key as Key) : body.message;
}

function errorBody(response: Response, payload: unknown): ErrorBody {
  let body: ErrorBody;
  if (payload && typeof payload === "object" && "code" in payload && "message" in payload) {
    body = { ...(payload as ErrorBody) };
  } else {
    const code = FALLBACK[response.status] ?? (response.status >= 500 ? "internal_error" : "http_error");
    body = has(`error.${code}`)
      ? { code, message: t(`error.${code}` as Key) }
      : { code, message: t("error.http", { status: response.status }), messageKey: "error.http", messageParams: { status: response.status } };
  }
  const retry = Number(response.headers.get("Retry-After"));
  if (response.status === 429 && retry > 0) body.retryAfter = retry;
  return body;
}

function queryString(query?: URLSearchParams | Record<string, string | undefined>): string {
  if (!query) return "";
  const params = query instanceof URLSearchParams ? query : new URLSearchParams();
  if (!(query instanceof URLSearchParams)) {
    for (const [k, v] of Object.entries(query)) if (v !== undefined && v !== "") params.set(k, v);
  }
  const s = params.toString();
  return s ? `?${s}` : "";
}
