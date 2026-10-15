// Protocol types: generated from Rust (types.gen.ts) plus the auth responses
// (rustcms-auth), which are checked against the server in the E2E suite.
export type * from "./types.gen";

/** Schema and vocabulary versions this bundle understands. */
export const SCHEMA_VERSION = 2;
export const VOCABULARY_VERSION = 7;

export interface Me {
  user: { id: string; name: string; email: string };
  guard: string;
  permissions: string[];
  superAdmin: boolean;
  /** Interface language for this user and browser. */
  locale?: string;
  /** The user's saved choice (`null`: follow the browser). */
  preferredLocale?: string | null;
  locales?: string[];
}

export type Data = Record<string, unknown>;
