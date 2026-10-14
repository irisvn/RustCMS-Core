// A secret setting: its stored state is shown, never its value. The user
// replaces or clears it explicitly; leaving it alone keeps it.
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import type { SecretStatus } from "@/api/types";
import { useT } from "@/i18n";

/** What saving does to a secret. */
export type SecretIntent = { kind: "keep" } | { kind: "replace"; value: string } | { kind: "clear" };

export interface SecretControl {
  status: Record<string, SecretStatus>;
  intents: Record<string, SecretIntent>;
  setIntent: (name: string, intent: SecretIntent) => void;
}

const KEEP: SecretIntent = { kind: "keep" };

export function SecretWidget({ id, name, control, disabled, invalid, describedBy, placeholder }: {
  id: string;
  name: string;
  control?: SecretControl;
  disabled?: boolean;
  invalid: boolean;
  describedBy?: string;
  placeholder?: string;
}) {
  const t = useT();
  const status = control?.status[name] ?? "unset";
  const intent = control?.intents[name] ?? KEEP;
  const badge =
    status === "set" ? <Badge>{t("secret.set")}</Badge>
      : status === "unreadable" ? <Badge className="border border-destructive bg-background text-destructive">{t("secret.unreadable")}</Badge>
        : <Badge className="border border-border bg-background text-muted-foreground">{t("secret.unset")}</Badge>;
  // Without a page that handles secrets (a newer form shown elsewhere), only the state.
  if (!control) return <div id={id} aria-describedby={describedBy}>{badge}</div>;
  const set = (next: SecretIntent) => control.setIntent(name, next);
  return (
    <div className="flex min-w-0 flex-col gap-2">
      <div className="flex flex-wrap items-center gap-2">
        {badge}
        {intent.kind === "clear" && <span className="text-xs text-muted-foreground">{t("secret.willClear")}</span>}
        {!disabled && intent.kind === "keep" && (
          <>
            <Button type="button" variant="outline" size="sm" onClick={() => set({ kind: "replace", value: "" })}>
              {status === "unset" ? t("secret.setValue") : t("secret.replace")}
            </Button>
            {status !== "unset" && (
              <Button type="button" variant="outline" size="sm" onClick={() => set({ kind: "clear" })}>{t("secret.clear")}</Button>
            )}
          </>
        )}
        {!disabled && intent.kind !== "keep" && (
          <Button type="button" variant="ghost" size="sm" onClick={() => set(KEEP)}>{t("secret.undo")}</Button>
        )}
      </div>
      {intent.kind === "replace" && (
        <Input
          id={id}
          type="password"
          autoComplete="new-password"
          value={intent.value}
          placeholder={placeholder ?? t("secret.placeholder")}
          disabled={disabled}
          aria-invalid={invalid || undefined}
          aria-describedby={describedBy}
          onChange={(e) => set({ kind: "replace", value: e.target.value })}
        />
      )}
    </div>
  );
}

/** The `data` entries of the secrets the user changed (`null` clears). */
export function secretPayload(intents: Record<string, SecretIntent>): Record<string, string | null> {
  const out: Record<string, string | null> = {};
  for (const [name, intent] of Object.entries(intents)) {
    if (intent.kind === "replace") out[name] = intent.value;
    else if (intent.kind === "clear") out[name] = null;
  }
  return out;
}
