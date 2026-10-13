import type { InputSchema, ResourceSchema } from "@/api/types";
import { defaultValues, fromFormValue, inputsOf, payloadOf, protectLossyJson, toFormValue, zodSchema } from "@/schema/form";
import create from "../../fixtures/contract/kitchen-sink.schema.create.json";
import edit from "../../fixtures/contract/kitchen-sink.schema.edit.json";
import records from "../../fixtures/contract/kitchen-sink.records.json";

const createSchema = create as unknown as ResourceSchema;
const editSchema = edit as unknown as ResourceSchema;
const input = (schema: ResourceSchema, name: string) => inputsOf(schema.form.components).find((i) => i.name === name)!;

describe("form values", () => {
  const inputs = inputsOf(editSchema.form.components);
  const record = records.data[0]!.data as Record<string, unknown>;

  it("round-trips every wire kind of the contract unchanged", () => {
    const values = defaultValues(inputs, record);
    const payload = payloadOf(inputs, values);
    for (const [name, value] of Object.entries(payload)) {
      if (name === "secret" || name === "local_at") continue;
      expect([name, value]).toEqual([name, record[name]]);
    }
    // datetime-local has minute precision; seconds are restored as :00.
    expect(payload.local_at).toBe("2026-10-12T09:30:00");
    expect(payload).not.toHaveProperty("secret");
  });

  it("sends only changed fields on edit", () => {
    const values = { ...defaultValues(inputs, record), name: "Renamed" };
    expect(payloadOf(inputs, values, { name: true })).toEqual({ name: "Renamed" });
  });

  it("sends write-only fields only when filled in", () => {
    const values = { ...defaultValues(inputs, record), secret: "s3cret" };
    expect(payloadOf(inputs, values, { secret: true })).toEqual({ secret: "s3cret" });
    expect(payloadOf(inputs, { ...values, secret: "" }, { secret: true })).toEqual({});
  });

  it("maps empty values by nullability", () => {
    const inputs = inputsOf(createSchema.form.components);
    const values = { ...defaultValues(inputs), name: "" };
    const payload = payloadOf(inputs, values);
    expect(input(createSchema, "notes").nullable).toBe(true);
    expect(payload.notes).toBeNull();
    expect(payload.name).toBe("");
    expect(payload.category_id).toBeNull();
    expect(payload.active).toBe(true); // declared default
  });

  it("measures select values, not their JSON encoding", async () => {
    const status = { ...input(createSchema, "status"), rules: [{ rule: "max_length" as const, value: 5 }] };
    const schema = zodSchema([status]);
    expect((await schema.safeParseAsync({ status: JSON.stringify("draft") })).success).toBe(true);
    expect((await schema.safeParseAsync({ status: JSON.stringify('a"b\\c') })).success).toBe(true);
    expect((await schema.safeParseAsync({ status: JSON.stringify("drafts") })).success).toBe(false);
  });

  it("keeps null, false and true apart for nullable booleans", () => {
    const flag = { ...input(editSchema, "active"), nullable: true };
    expect(toFormValue(flag, null)).toBe("");
    expect(toFormValue(flag, false)).toBe("false");
    expect(fromFormValue(flag, "false")).toBe(false);
    expect(fromFormValue(flag, "")).toBeNull();
    // From null to an explicit "No" is a change, so it is submitted.
    expect(payloadOf([flag], { active: "false" }, { active: true })).toEqual({ active: false });
  });

  it("never writes back JSON whose integers lost precision", async () => {
    const colors = { ...input(createSchema, "notes"), valueType: "json" };
    const schema = zodSchema([colors]);
    expect((await schema.safeParseAsync({ notes: '{"account": 9007199254740993}' })).success).toBe(false);
    expect((await schema.safeParseAsync({ notes: '{"account": "9007199254740993", "n": 42}' })).success).toBe(true);
    // A stored value already rounded by the browser is shown read-only.
    const [protectedInput] = protectLossyJson([colors], { notes: { account: 9007199254740992, label: "x" } });
    expect((protectedInput as typeof colors).disabled).toBe(true);
    const [editable] = protectLossyJson([colors], { notes: { account: 42 } });
    expect((editable as typeof colors).disabled).toBe(false);
  });

  it("rejects lossy numbers in every JSON-valued input", async () => {
    // What the browser holds after parsing 9007199254740993 from the server.
    const big = JSON.parse("9007199254740993") as number;
    const json = { ...input(createSchema, "notes"), valueType: "json" };
    expect((await zodSchema([json]).safeParseAsync({ notes: '{"amount": 1e400}' })).success).toBe(false);
    const status = { ...input(createSchema, "status"), valueType: "json", options: [{ value: big, label: "Big" }] };
    expect((await zodSchema([status]).safeParseAsync({ status: JSON.stringify(big) })).success).toBe(false);
    expect((await zodSchema([status]).safeParseAsync({ status: JSON.stringify(42) })).success).toBe(true);
    const colors = input(createSchema, "colors");
    expect((await zodSchema([colors]).safeParseAsync({ colors: [JSON.stringify(big)] })).success).toBe(false);
    expect((await zodSchema([colors]).safeParseAsync({ colors: [JSON.stringify("red")] })).success).toBe(true);
  });

  it("starts new records from declared defaults", () => {
    const values = defaultValues(inputsOf(createSchema.form.components));
    expect(input(createSchema, "active").default).toBe(true);
    expect(values.active).toBe(true);
    expect(values.notes).toBe("");
  });

  it("leaves unset hidden inputs to the server", () => {
    const inputs = inputsOf(createSchema.form.components);
    expect(input(createSchema, "token").type).toBe("hidden");
    expect(payloadOf(inputs, defaultValues(inputs))).not.toHaveProperty("token");
    expect(payloadOf(inputs, { ...defaultValues(inputs), token: "0199d3a4-1b2c-7d3e-8f40-123456789abc" })).toHaveProperty("token");
  });

  it("never blocks a submit on a hidden input the user cannot fill", async () => {
    const token = { ...input(createSchema, "token"), rules: [{ rule: "required" }] } as InputSchema;
    expect((await zodSchema([token]).safeParseAsync({ token: "" })).success).toBe(true);
    // Reverse check: the same rule on a visible input is enforced.
    const visible = { ...token, type: "text_input" } as InputSchema;
    expect((await zodSchema([visible]).safeParseAsync({ token: "" })).success).toBe(false);
  });

  it("never submits disabled or unknown inputs", () => {
    const fake: InputSchema[] = [
      { ...input(editSchema, "name"), disabled: true },
      { ...input(editSchema, "notes"), type: "future_widget" as InputSchema["type"] },
    ];
    expect(payloadOf(fake, { name: "x", notes: "y" })).toEqual({});
  });

  it("validates rules like the server", async () => {
    const schema = zodSchema(inputsOf(createSchema.form.components));
    const values = defaultValues(inputsOf(createSchema.form.components));
    const issues = async (changes: Record<string, unknown>) => {
      const result = await schema.safeParseAsync({ ...values, ...changes });
      return result.success ? {} : Object.fromEntries(result.error.issues.map((i) => [i.path[0], i.message]));
    };
    expect(await issues({ name: "" })).toHaveProperty("name", "is required");
    // max_length counts characters (Rust chars), not UTF-16 units: 60 emoji fit.
    expect(await issues({ name: "😀".repeat(60) })).not.toHaveProperty("name");
    expect(await issues({ name: "😀".repeat(61) })).toHaveProperty("name");
    expect(await issues({ name: "ok", serial: "1.5" })).toHaveProperty("serial");
    expect(await issues({ name: "ok", price: "10.5" })).toEqual({});
  });
});
