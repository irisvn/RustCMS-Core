import { afterEach, describe, expect, it } from "vitest";
import issues from "../../fixtures/contract/issues.json";
import { en } from "@/i18n/en";
import { vi } from "@/i18n/vi";
import { LOCALES, interpolate, issueText, pickLocale, setLocale, t, translate } from "@/i18n";
import { protectLossyJson, relabel } from "@/schema/form";
import { ApiError } from "@/api/client";
import { failure } from "@/pages/system/shared";
import { chooseLocale, serverLocale } from "@/i18n/preference";
import { getLocale } from "@/i18n";
import type { ComponentSchema } from "@/api/types";

const placeholders = (text: string) => [...text.matchAll(/\{(\w+)\}/g)].map((m) => m[1]).sort();

afterEach(() => setLocale("en"));

describe("catalogs", () => {
  it("translate every key with the same placeholders", () => {
    for (const [key, text] of Object.entries(en)) {
      const translated = (vi as Record<string, string>)[key] ?? "";
      expect(translated, key).toBeTypeOf("string");
      expect(translated.trim(), key).not.toBe("");
      expect(placeholders(translated), key).toEqual(placeholders(text));
    }
    expect(Object.keys(vi).sort()).toEqual(Object.keys(en).sort());
  });

  it("have the plural forms each language needs", () => {
    const bases = Object.keys(en).filter((k) => k.endsWith("_other")).map((k) => k.slice(0, -"_other".length));
    for (const locale of LOCALES) {
      const forms = new Intl.PluralRules(locale).resolvedOptions().pluralCategories;
      for (const base of bases) {
        for (const form of forms) {
          // `other` is the fallback of every form; English also needs `one`.
          if (form === "other" || (locale === "en" && form === "one")) expect(`${base}_${form}` in en, `${base}_${form}`).toBe(true);
        }
      }
    }
  });

  it("cover every server issue code, with its parameters", () => {
    for (const [code, templates] of Object.entries(issues as Record<string, string[]>)) {
      const key = `issue.${code}` as keyof typeof en;
      expect(en[key], code).toBeTypeOf("string");
      // A translation uses only parameters the server sends for that code.
      const sent = new Set(templates.flatMap(placeholders));
      for (const name of placeholders(vi[key])) expect(sent.has(name), `${code}: {${name}}`).toBe(true);
    }
    const known = new Set(Object.keys(issues).map((code) => `issue.${code}`));
    const stale = Object.keys(en).filter((k) => k.startsWith("issue.") && !known.has(k));
    expect(stale).toEqual([]);
  });
});

describe("translate", () => {
  it("interpolates once, as text", () => {
    expect(interpolate("{a} and {b}", { a: "{b}", b: "<i>x</i>" })).toBe("{b} and <i>x</i>");
    expect(interpolate("{missing}", {})).toBe("{missing}");
  });

  it("picks plural forms and falls back to English", () => {
    expect(translate("en", "list.records", { count: 1, n: "1" })).toBe("1 record");
    expect(translate("en", "list.records", { count: 2, n: "2" })).toBe("2 records");
    expect(translate("vi", "list.records", { count: 2, n: "2" })).toBe("2 bản ghi");
    setLocale("vi");
    expect(t("action.delete")).toBe("Xoá");
    expect(document.documentElement.lang).toBe("vi");
  });

  it("translates server issues by code, else shows the server's text", () => {
    setLocale("vi");
    expect(issueText({ code: "max_length", params: { max: "200" } }, "may not be longer than 200 characters")).toBe(
      "không được dài quá 200 ký tự",
    );
    expect(issueText({ code: "custom", params: { message: "Mine" } }, "Mine")).toBe("Mine");
    expect(issueText({ code: "from_a_newer_server" }, "English text")).toBe("English text");
    expect(issueText(undefined, "English text")).toBe("English text");
  });

  it("chooses among the offered languages", () => {
    expect(pickLocale(["vi-VN", "en"], ["en", "vi"], "en")).toBe("vi");
    expect(pickLocale(["fr"], ["en", "vi"], "vi")).toBe("vi");
    expect(pickLocale(["vi"], ["en"], "en")).toBe("en");
    expect(pickLocale([], ["en", "vi"], "xx")).toBe("en");
  });
});

describe("relabel", () => {
  const input = (label: string, help?: string) =>
    ({ type: "text_input", name: "title", label, help, valueType: "string", nullable: false, rules: [], disabled: false, writeOnly: false }) as unknown as ComponentSchema;
  const tabs = (label: string, child: ComponentSchema) =>
    ({ type: "tabs", components: [{ type: "tab", id: "content", label, components: [child] }] }) as unknown as ComponentSchema;

  it("takes the new labels and keeps the form", () => {
    const shown = [tabs("Content", input("Title", "Short"))];
    const latest = [tabs("Nội dung", input("Tiêu đề", "Ngắn"))];
    const out = relabel(shown, latest) as unknown as { components: { label: string; components: { label: string; help: string }[] }[] }[];
    expect(out[0]!.components[0]!.label).toBe("Nội dung");
    expect(out[0]!.components[0]!.components[0]).toMatchObject({ label: "Tiêu đề", help: "Ngắn" });
    // A different structure keeps what is shown.
    expect(relabel(shown, [])).toEqual(shown);
  });
});

describe("language changes", () => {
  it("notice someone else's change by its code, in any language", () => {
    const stale = new ApiError(422, {
      message: "The given data was invalid.", code: "validation_failed",
      formErrors: ["This was changed by someone else meanwhile. Reload and try again."],
      formIssues: [{ code: "stale", params: {} }],
    });
    for (const locale of LOCALES) {
      setLocale(locale);
      expect(failure(stale).stale, locale).toBe(true);
    }
    setLocale("vi");
    expect(failure(stale).form).toBe("Dữ liệu vừa được người khác thay đổi. Hãy tải lại và thử lại.");
  });

  it("read error messages in the current language, also later", () => {
    const error = new ApiError(429, { message: "x", code: "too_many_requests", retryAfter: 5 });
    expect(error.message).toBe("Too many attempts. Try again in 5 s.");
    setLocale("vi");
    expect(error.message).toBe("Thử quá nhiều lần. Hãy thử lại sau 5 giây.");
  });

  it("keep why an input is read-only when relabelling", () => {
    const input = { type: "key_value", name: "specs", label: "Specs", help: "Pairs", valueType: "json", nullable: true, rules: [], disabled: false, writeOnly: false } as unknown as ComponentSchema;
    const shown = protectLossyJson([input], { specs: ["not", "pairs"] });
    setLocale("vi");
    const relabelled = relabel(shown, [{ ...(input as object), label: "Thông số", help: "Các cặp" } as ComponentSchema]) as unknown as { help: string; label: string; disabled: boolean }[];
    expect(relabelled[0]).toMatchObject({ label: "Thông số", disabled: true, help: "Chứa giá trị không phải cặp chữ: không sửa được ở đây." });
  });

  it("ignore an older /me once a language was chosen", async () => {
    const before = Date.now() - 1_000;
    await chooseLocale("vi", async () => {});
    serverLocale("en", before);
    expect(getLocale()).toBe("vi");
    serverLocale("en", Date.now() + 1);
    expect(getLocale()).toBe("en");
  });
});
