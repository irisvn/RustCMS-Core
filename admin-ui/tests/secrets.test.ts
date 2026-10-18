import { describe, expect, it } from "vitest";
import type { InputSchema } from "@/api/types";
import { defaultValues, payloadOf } from "@/schema/form";
import { parseSecretMap, secretMapOperation } from "@/schema/secrets";

const input = (name: string, type: string, valueType: string): InputSchema =>
  ({ type, name, label: name, valueType, nullable: true, disabled: false, writeOnly: true, rules: [], columnSpan: 1 }) as InputSchema;

const token = input("token", "secret", "secret");
const env = input("env", "secret_map", "secret_map");
const record = {
  token: { state: "set" },
  env: { state: "set", keys: ["A", "B"], revision: "r1" },
};

describe("encrypted fields in resource forms", () => {
  it("never prefill values and send nothing when unchanged", () => {
    const values = defaultValues([token, env], record);
    expect(JSON.stringify(values)).not.toContain("value\":\"x");
    expect(parseSecretMap(values.env).rows.map((r) => [r.key, r.value])).toEqual([["A", ""], ["B", ""]]);
    expect(payloadOf([token, env], values)).toEqual({});
  });

  it("send keep by name, set for new or replaced keys, and the revision", () => {
    const values = defaultValues([token, env], record);
    const map = parseSecretMap(values.env);
    map.rows[0]!.action = "replace";
    map.rows[0]!.value = "new-a";
    map.rows.push({ key: "C", stored: false, action: "replace", value: "c" });
    values.env = JSON.stringify(map);
    values.token = JSON.stringify({ state: "set", intent: { kind: "replace", value: "t2" } });
    expect(payloadOf([token, env], values)).toEqual({
      token: { replace: "t2" },
      env: { revision: "r1", keep: ["B"], set: { A: "new-a", C: "c" } },
    });
  });

  it("remove stored keys by leaving them out; clear a string explicitly", () => {
    const values = defaultValues([token, env], record);
    const map = parseSecretMap(values.env);
    map.rows[1]!.action = "remove";
    values.env = JSON.stringify(map);
    values.token = JSON.stringify({ state: "set", intent: { kind: "clear" } });
    expect(payloadOf([token, env], values)).toEqual({ token: { clear: true }, env: { revision: "r1", keep: ["A"], set: {} } });
  });

  it("create forms carry no revision and no keep", () => {
    const values = defaultValues([token, env]);
    const map = parseSecretMap(values.env);
    expect(map.state).toBe("unset");
    map.rows.push({ key: "A", stored: false, action: "replace", value: "1" });
    expect(secretMapOperation(JSON.stringify(map))).toEqual({ set: { A: "1" } });
  });

  it("settings secrets are still sent from their own intents", () => {
    const setting = input("smtp_password", "secret", "string");
    expect(payloadOf([setting], { smtp_password: "" })).toEqual({});
  });
});

describe("clearing an encrypted map", () => {
  it("sends clear with the revision, and only for a stored map", () => {
    const values = defaultValues([env], record);
    const map = parseSecretMap(values.env);
    expect(secretMapOperation(JSON.stringify({ ...map, clear: true }))).toEqual({ revision: "r1", clear: true });
    const fresh = parseSecretMap(defaultValues([env]).env);
    expect(secretMapOperation(JSON.stringify({ ...fresh, clear: true }))).toBeUndefined();
  });
});
