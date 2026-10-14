import { useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { useSession } from "@/api/context";
import { keys } from "@/api/queries";
import { NativeSelect } from "@/components/ui/input";
import { errorText, isLocale, LOCALES, show, type Text, useLocale, useT } from "@/i18n";
import { chooseLocale } from "@/i18n/preference";

/** Language picker: saved on the account when signed in, on this device otherwise. */
export function LanguageSelect({ id = "language", signedIn = false, labelled = false }: {
  id?: string;
  signedIn?: boolean;
  /** A visible label elsewhere names the select. */
  labelled?: boolean;
}) {
  const t = useT();
  const locale = useLocale();
  const session = useSession();
  const client = useQueryClient();
  const [error, setError] = useState<Text | null>(null);
  const offered = LOCALES.filter((l) => session.config.locales?.includes(l) ?? true);
  if (offered.length < 2) return null;
  const change = async (value: string) => {
    if (!isLocale(value)) return;
    setError(null);
    try {
      await chooseLocale(value, signedIn ? (l) => session.api.post("/auth/locale", { locale: l }) : undefined);
      if (signedIn) await client.invalidateQueries({ queryKey: keys.me });
    } catch (e) {
      setError(() => errorText(e, "state.errorText"));
    }
  };
  return (
    <div className="flex flex-col gap-1">
      {!labelled && <label htmlFor={id} className="sr-only">{t("language.label")}</label>}
      <NativeSelect id={id} value={locale} onChange={(e) => void change(e.target.value)} className="h-9 w-auto">
        {offered.map((l) => <option key={l} value={l}>{t(`language.${l}`)}</option>)}
      </NativeSelect>
      {error && <p role="alert" className="text-xs text-destructive">{show(error)}</p>}
    </div>
  );
}
