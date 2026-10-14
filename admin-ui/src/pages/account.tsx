import { type FormEvent, useState } from "react";
import { ApiError } from "@/api/client";
import { useUncertain } from "@/api/context";
import { ACCOUNT, useChangePassword, useLogoutOthers } from "@/api/system";
import { usePanel } from "@/api/queries";
import { Page, PageHeader } from "@/components/layout";
import { PausedBanner } from "@/components/states";
import { Alert } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Input, Label } from "@/components/ui/input";
import { LanguageSelect } from "@/components/language";
import { later, show, t as translate, type Text, useT } from "@/i18n";

type Notice = { variant: "default" | "destructive"; text: Text } | null;

function errorsOf(e: unknown): { fields: Record<string, string[]>; text: string | null } {
  if (e === null) return { fields: {}, text: null };
  if (e instanceof ApiError && e.status === 422) {
    // Form-level messages have no field to sit under; with neither, the message says why.
    const text = e.formErrors.length ? e.formErrors.join(" ") : Object.keys(e.fieldErrors).length ? null : e.message;
    return { fields: e.fieldErrors, text };
  }
  return { fields: {}, text: e instanceof Error ? e.message : translate("state.errorText") };
}

/** The signed-in user's own account: password and other sessions. */
export function AccountPage() {
  const t = useT();
  const panel = usePanel();
  const change = useChangePassword();
  const others = useLogoutOthers();
  const [pw, setPw] = useState({ current_password: "", password: "", password_confirmation: "" });
  const [confirmPw, setConfirmPw] = useState("");
  // Each form keeps its own errors: both send the password as `current_password`.
  // Failures are kept as they are: their messages are read in the current language.
  const [pwFailure, setPwFailure] = useState<unknown>(null);
  const [otherFailure, setOtherFailure] = useState<unknown>(null);
  const pwFields = errorsOf(pwFailure).fields;
  const otherErrors = errorsOf(otherFailure).fields;
  const otherMessage = otherErrors.current_password ?? otherErrors.password;
  const otherFields = otherMessage ? { confirm_password: otherMessage } : otherErrors;
  const [notice, setNotice] = useState<Notice>(null);
  // After a password change with an unknown outcome, repeating it could fail confusingly or sign out.
  const paused = useUncertain(ACCOUNT);

  const submitPassword = async (e: FormEvent) => {
    e.preventDefault();
    setPwFailure(null);
    setNotice(null);
    try {
      await change.mutateAsync(pw);
      setPw({ current_password: "", password: "", password_confirmation: "" });
      setNotice({ variant: "default", text: later("account.passwordChanged") });
    } catch (err) {
      setPwFailure(err);
      if (errorsOf(err).text) setNotice({ variant: "destructive", text: () => errorsOf(err).text ?? "" });
    }
  };
  const submitOthers = async (e: FormEvent) => {
    e.preventDefault();
    setOtherFailure(null);
    setNotice(null);
    try {
      await others.mutateAsync(confirmPw);
      setConfirmPw("");
      setNotice({ variant: "default", text: later("account.othersSignedOut") });
    } catch (err) {
      setOtherFailure(err);
      if (errorsOf(err).text) setNotice({ variant: "destructive", text: () => errorsOf(err).text ?? "" });
    }
  };
  const error = (fields: Record<string, string[]>, name: string, id: string) =>
    fields[name] && <p id={id} role="alert" className="text-xs text-destructive">{fields[name]!.join(" ")}</p>;

  return (
    <Page>
      <PageHeader title={t("account.title")} description={t("account.description")}
        eyebrow={t("account.eyebrow")} />
      <PausedBanner scope={ACCOUNT} />
      {notice && <Alert variant={notice.variant}>{show(notice.text)}</Alert>}
      <div className="grid min-w-0 items-start gap-6 lg:grid-cols-[minmax(0,1fr)_minmax(0,2fr)]">
        <Card className="min-w-0">
          <CardHeader>
            <CardTitle>{t("account.profile")}</CardTitle>
            <p className="text-sm text-muted-foreground">{t("account.profileText")}</p>
          </CardHeader>
          <CardContent>
            {panel.data && (
              <dl className="flex flex-col gap-5 text-sm">
                <div className="min-w-0">
                  <dt className="text-muted-foreground">{t("field.name")}</dt>
                  <dd className="mt-1 break-words font-medium">{panel.data.user.name}</dd>
                </div>
                <div className="min-w-0">
                  <dt className="text-muted-foreground">{t("field.email")}</dt>
                  <dd className="mt-1 break-all font-medium">{panel.data.user.email}</dd>
                </div>
                <div className="min-w-0">
                  <dt className="text-muted-foreground"><label htmlFor="account-language">{t("language.label")}</label></dt>
                  <dd className="mt-1"><LanguageSelect id="account-language" signedIn labelled /></dd>
                </div>
              </dl>
            )}
          </CardContent>
        </Card>
        <section aria-label={t("account.security")} className="flex min-w-0 flex-col gap-6">
          <Card>
            <CardHeader>
              <CardTitle>{t("account.changePassword")}</CardTitle>
              <p id="password-help" className="text-sm text-muted-foreground">{t("account.changePasswordText")}</p>
            </CardHeader>
            <CardContent>
              <form className="flex max-w-lg flex-col gap-5" onSubmit={(e) => void submitPassword(e)}
                noValidate aria-describedby="password-help">
                <div className="flex flex-col gap-2">
                  <Label htmlFor="current-password">{t("account.currentPassword")}</Label>
                  <Input id="current-password" type="password" autoComplete="current-password"
                    value={pw.current_password}
                    onChange={(e) => setPw({ ...pw, current_password: e.target.value })}
                    disabled={change.isPending} aria-invalid={!!pwFields.current_password}
                    aria-describedby={pwFields.current_password ? "current-password-error" : undefined} />
                  {error(pwFields, "current_password", "current-password-error")}
                </div>
                <div className="flex flex-col gap-2">
                  <Label htmlFor="new-password">{t("account.newPassword")}</Label>
                  <Input id="new-password" type="password" autoComplete="new-password"
                    value={pw.password}
                    onChange={(e) => setPw({ ...pw, password: e.target.value })}
                    disabled={change.isPending} aria-invalid={!!pwFields.password}
                    aria-describedby={pwFields.password ? "new-password-error" : undefined} />
                  {error(pwFields, "password", "new-password-error")}
                </div>
                <div className="flex flex-col gap-2">
                  <Label htmlFor="confirm-new-password">{t("account.confirmPassword")}</Label>
                  <Input id="confirm-new-password" type="password" autoComplete="new-password"
                    value={pw.password_confirmation}
                    onChange={(e) => setPw({ ...pw, password_confirmation: e.target.value })}
                    disabled={change.isPending} aria-invalid={!!pwFields.password_confirmation}
                    aria-describedby={pwFields.password_confirmation ? "confirm-new-password-error" : undefined} />
                  {error(pwFields, "password_confirmation", "confirm-new-password-error")}
                </div>
                <div className="border-t border-border pt-5">
                  <Button type="submit" disabled={paused || change.isPending}>
                    {t("account.changePassword")}
                  </Button>
                </div>
              </form>
            </CardContent>
          </Card>
          <Card>
            <CardHeader>
              <CardTitle>{t("account.otherSessions")}</CardTitle>
            </CardHeader>
            <CardContent>
              <form className="flex max-w-lg flex-col gap-5" onSubmit={(e) => void submitOthers(e)}
                noValidate aria-describedby="sessions-help">
                <p id="sessions-help" className="text-sm leading-6 text-muted-foreground">{t("account.otherSessionsText")}</p>
                <div className="flex flex-col gap-2">
                  <Label htmlFor="confirm-password">{t("field.password")}</Label>
                  <Input id="confirm-password" type="password" autoComplete="current-password"
                    value={confirmPw} onChange={(e) => setConfirmPw(e.target.value)}
                    disabled={others.isPending} aria-invalid={!!otherFields.confirm_password}
                    aria-describedby={otherFields.confirm_password ? "sessions-help confirm-password-error" : "sessions-help"} />
                  {error(otherFields, "confirm_password", "confirm-password-error")}
                </div>
                <div className="border-t border-border pt-5">
                  <Button type="submit" variant="outline" disabled={paused || !confirmPw || others.isPending}>
                    {t("account.signOutOthers")}
                  </Button>
                </div>
              </form>
            </CardContent>
          </Card>
        </section>
      </div>
    </Page>
  );
}
