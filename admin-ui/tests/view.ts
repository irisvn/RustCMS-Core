// For tests written against form schemas: the view page the server makes from
// a form (`from_form`, crates/rustcms-admin/src/engine/adapter/infolist.rs),
// so they draw a record as the panel does. view-parity.test.tsx checks this
// stays the server's own mapping on the contract fixtures.
import type { ComponentSchema, EntrySchema, InfolistSchema, InputSchema } from "@/api/types";
import { isInput, isLayout } from "@/schema/form";

const SKIPPED = new Set(["hidden", "belongs_to_many", "secret", "secret_map"]);

function entryOf(input: InputSchema): EntrySchema | null {
  if (SKIPPED.has(input.type) || input.writeOnly || input.valueType === "rows") return null;
  const base = { name: input.name, label: input.label, valueType: input.valueType, columnSpan: input.columnSpan, shownWhen: input.shownWhen };
  const as = (type: string, more: Partial<EntrySchema> = {}) => ({ ...base, type, ...more }) as EntrySchema;
  if (input.type === "repeater" && input.repeater) return as("repeatable_entry", { components: viewOf(input.repeater.components) });
  if (input.type === "select" && input.relationship) return as("relation_entry", { relationship: input.relationship });
  if ((input.type === "toggle" || input.type === "checkbox") && input.valueType === "bool") return as("boolean_entry");
  if (input.type === "key_value" && input.valueType === "json") return as("key_value_entry");
  // The server falls back to text for an upload on a field that is not a media field
  // (no `media` in the input's schema). A rich editor on a field that is not rich text
  // falls back too, which an input's schema does not tell: tests use rich text fields.
  // The upload's viewer switch goes with it (vocabulary 22).
  if (input.type === "file_upload" && input.media) return as("media_entry", input.media.preview === false ? { preview: false } : {});
  if (input.type === "rich_editor") return as("rich_text_entry", { richNodes: input.richNodes });
  const options = input.options?.length ? input.options : undefined;
  const text = input.type === "text_input";
  return as("text_entry", {
    multiline: true,
    options,
    list: input.type === "checkbox_list" || undefined,
    password: (text && input.mode === "password") || undefined,
    prefix: text ? input.prefix : undefined,
    suffix: text ? input.suffix : undefined,
  });
}

/** The entries of `components`, in their layouts (empty layouts left out). */
export function viewOf(components: ComponentSchema[]): ComponentSchema[] {
  return components.flatMap((c): ComponentSchema[] => {
    if (isLayout(c)) {
      const children = viewOf(c.components);
      return children.length ? [{ ...c, components: children }] : [];
    }
    const entry = isInput(c) ? entryOf(c) : null;
    return entry ? [entry] : [];
  });
}

/** The infolist of a form's components. */
export const infolistOf = (components: ComponentSchema[], columns = 1): InfolistSchema => ({ columns, components: viewOf(components) });
