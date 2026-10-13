// Protocol types: generated from Rust (types.gen.ts) plus the auth responses
// (rustcms-auth), which are checked against the server in the E2E suite.
export type * from "./types.gen";

/** Schema and vocabulary versions this bundle understands. */
export const SCHEMA_VERSION = 1;
export const VOCABULARY_VERSION = 3;

export interface Me {
  user: { id: string; name: string; email: string };
  guard: string;
  permissions: string[];
  superAdmin: boolean;
}

export type Data = Record<string, unknown>;
