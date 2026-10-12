// HTTP client of the admin API: same-origin cookies, CSRF header on writes,
// typed errors. Writes are never retried automatically.

export interface ErrorBody {
  message: string;
  code: string;
  errors?: Record<string, string[]>;
  formErrors?: string[];
}

export class ApiError extends Error {
  readonly status: number;
  readonly body: ErrorBody;

  constructor(status: number, body: ErrorBody) {
    super(body.message);
    this.status = status;
    this.body = body;
  }

  get code() {
    return this.body.code;
  }

  get fieldErrors(): Record<string, string[]> {
    return this.body.errors ?? {};
  }

  get formErrors(): string[] {
    return this.body.formErrors ?? [];
  }

  /** A write whose outcome is unknown: resubmitting could apply it twice. */
  get maybeApplied(): boolean {
    return ["outcome_unknown", "incomplete_response"].includes(this.code) ||
      (this.code === "network_error" && this.message.includes("may have been applied"));
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

  /** The session's CSRF token, fetched once and kept in memory only. */
  async csrfToken(): Promise<string> {
    if (this.csrf) return this.csrf;
    const generation = this.generation;
    this.csrfLoading ??= this.request<{ csrfToken: string; userId?: string | null }>("GET", "/auth/csrf").then((r) => {
      // The session was reset while the token was on its way: it may belong to someone else.
      if (generation !== this.generation) throw sessionReset();
      // A token is only used for the account this page was built for.
      if (this.opts.verifyIdentity && !this.opts.verifyIdentity(r.userId ?? null)) {
        throw new ApiError(409, {
          code: "session_changed",
          message: "You are now signed in as someone else in this browser. The page was reloaded; nothing was saved.",
        });
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

  async request<T>(method: string, path: string, body?: unknown): Promise<T> {
    const headers: Record<string, string> = { Accept: "application/json" };
    if (body !== undefined) headers["Content-Type"] = "application/json";
    const generation = this.generation;
    // An answer to a request from an earlier session state (before a sign-in,
    // sign-out or account change) never reaches identity or auth handling: it
    // could otherwise clear or redirect the current session.
    const obsolete = () => {
      if (generation === this.generation) return null;
      return SAFE.has(method)
        ? sessionReset()
        : new ApiError(409, {
            code: "outcome_unknown",
            message: "Your session changed while this was being saved; it may have been applied. Reload to check.",
          });
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
        body: body === undefined ? undefined : JSON.stringify(body),
        credentials: "same-origin",
        cache: "no-store",
      });
    } catch {
      // A write may have reached the server before the connection failed: never claim it did not.
      const message = SAFE.has(method)
        ? "The server could not be reached."
        : "The connection failed; the change may have been applied. Refresh to check.";
      throw new ApiError(0, { message, code: "network_error" });
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
      const message = SAFE.has(method)
        ? "The server's answer was incomplete. Please try again."
        : "The change may have been applied, but the answer was incomplete. Refresh before trying again.";
      // Only a sent write can have been applied; a cut-off read is just a failed read.
      throw new ApiError(response.status, { message, code: SAFE.has(method) ? "incomplete_read" : "incomplete_response" });
    }
    if (!response.ok && !SAFE.has(method) && (response.status >= 500 || response.status === 408)) {
      // A server error, timeout or gateway error says nothing about whether the
      // write committed (e.g. the connection dropped while acknowledging COMMIT).
      throw new ApiError(response.status, {
        code: "outcome_unknown",
        message: "The server did not confirm the change; it may have been applied. Refresh before trying again.",
      });
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
        error.body.message = "Your session was refreshed. Please try again.";
      }
      throw error;
    }
    return payload as T;
  }
}

export const sessionReset = () =>
  new ApiError(409, { code: "session_changed", message: "Your session changed before this was sent; nothing was saved." });

const FALLBACK: Record<number, [string, string]> = {
  400: ["bad_request", "The request was malformed."],
  404: ["not_found", "Not found."],
  405: ["method_not_allowed", "Method not allowed."],
  408: ["timeout", "The server took too long to answer."],
  413: ["payload_too_large", "The data is too large."],
  429: ["too_many_requests", "Too many attempts. Please try again later."],
};

function errorBody(response: Response, payload: unknown): ErrorBody {
  let body: ErrorBody;
  if (payload && typeof payload === "object" && "code" in payload && "message" in payload) {
    body = { ...(payload as ErrorBody) };
  } else {
    const [code, message] = FALLBACK[response.status] ??
      (response.status >= 500 ? ["internal_error", "Server error."] : ["http_error", `Request failed (${response.status}).`]);
    body = { code, message };
  }
  const retry = Number(response.headers.get("Retry-After"));
  if (response.status === 429 && retry > 0) body.message = `Too many attempts. Try again in ${retry} s.`;
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
