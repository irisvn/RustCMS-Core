// The Mail settings page's test: sends a mail now with the saved settings.
import { useId, useState } from "react";
import { ApiError } from "@/api/client";
import { useMailTest } from "@/api/settings";
import { Alert } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { type Key, errorText, show, type Text, useT } from "@/i18n";

const CODES: Record<string, Key> = {
  mail_disabled: "mail.error.mail_disabled",
  mail_not_configured: "mail.error.mail_not_configured",
  mail_policy: "mail.error.mail_policy",
  mail_settings_unreadable: "mail.error.mail_settings_unreadable",
  mail_invalid: "mail.error.mail_invalid",
  mail_template: "mail.error.mail_template",
  smtp_connect: "mail.error.smtp_connect",
  smtp_tls: "mail.error.smtp_tls",
  smtp_auth: "mail.error.smtp_auth",
  smtp_rejected: "mail.error.smtp_rejected",
  smtp_temporary: "mail.error.smtp_temporary",
  smtp_timeout: "mail.error.smtp_timeout",
  smtp_throttled: "mail.error.smtp_throttled",
};

export function MailTest() {
  const t = useT();
  const test = useMailTest();
  const [to, setTo] = useState("");
  const [result, setResult] = useState<{ ok: boolean; text: Text } | null>(null);
  const id = useId();
  const send = async () => {
    setResult(null);
    try {
      const outcome = await test.mutateAsync(to.trim());
      setResult(
        outcome.ok
          ? { ok: true, text: () => t("mail.test.sent", { to: to.trim() }) }
          : { ok: false, text: () => t(CODES[outcome.code ?? ""] ?? "mail.error.unknown") },
      );
    } catch (e) {
      const field = e instanceof ApiError ? e.fieldErrors.to?.join(" ") : undefined;
      setResult({ ok: false, text: field ? () => field : errorText(e, "mail.error.unknown") });
    }
  };
  return (
    <Card>
      <CardHeader>
        <CardTitle>{t("mail.test.title")}</CardTitle>
      </CardHeader>
      <CardContent className="flex flex-col gap-3">
        <p className="text-sm text-muted-foreground">{t("mail.test.description")}</p>
        <div className="flex flex-wrap items-center gap-2">
          <label htmlFor={id} className="sr-only">{t("mail.test.to")}</label>
          <Input
            id={id}
            type="email"
            className="max-w-xs"
            placeholder={t("mail.test.to")}
            value={to}
            onChange={(e) => setTo(e.target.value)}
          />
          <Button type="button" variant="outline" disabled={test.isPending || to.trim() === ""} onClick={() => void send()}>
            {test.isPending ? t("mail.test.sending") : t("mail.test.send")}
          </Button>
        </div>
        {result && <Alert variant={result.ok ? "default" : "destructive"}>{show(result.text)}</Alert>}
      </CardContent>
    </Card>
  );
}
