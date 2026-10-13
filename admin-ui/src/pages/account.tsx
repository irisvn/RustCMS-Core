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

type Notice = { variant: "default" | "destructive"; text: string } | null;

function errorsOf(e: unknown): { fields: Record<string, string[]>; text: string | null } {
  if (e instanceof ApiError && e.status === 422) {
    // Form-level messages have no field to sit under; with neither, the message says why.
    const text = e.formErrors.length ? e.formErrors.join(" ") : Object.keys(e.fieldErrors).length ? null : e.message;
    return { fields: e.fieldErrors, text };
  }
  return { fields: {}, text: e instanceof Error ? e.message : "Something went wrong." };
}

/** The signed-in user's own account: password and other sessions. */
export function AccountPage() {
  const panel = usePanel();
  const change = useChangePassword();
  const others = useLogoutOthers();
  const [pw, setPw] = useState({ current_password: "", password: "", password_confirmation: "" });
  const [confirmPw, setConfirmPw] = useState("");
  // Each form keeps its own errors: both send the password as `current_password`.
  const [pwFields, setPwFields] = useState<Record<string, string[]>>({});
  const [otherFields, setOtherFields] = useState<Record<string, string[]>>({});
  const [notice, setNotice] = useState<Notice>(null);
  // After a password change with an unknown outcome, repeating it could fail confusingly or sign out.
  const paused = useUncertain(ACCOUNT);

  const submitPassword = async (e: FormEvent) => {
    e.preventDefault();
    setPwFields({});
    setNotice(null);
    try {
      await change.mutateAsync(pw);
      setPw({ current_password: "", password: "", password_confirmation: "" });
      setNotice({ variant: "default", text: "Password changed. Your other sessions were signed out." });
    } catch (err) {
      const { fields, text } = errorsOf(err);
      setPwFields(fields);
      if (text) setNotice({ variant: "destructive", text });
    }
  };
  const submitOthers = async (e: FormEvent) => {
    e.preventDefault();
    setOtherFields({});
    setNotice(null);
    try {
      await others.mutateAsync(confirmPw);
      setConfirmPw("");
      setNotice({ variant: "default", text: "Signed out everywhere else; your API tokens were revoked." });
    } catch (err) {
      const { fields, text } = errorsOf(err);
      const message = fields.current_password ?? fields.password;
      setOtherFields(message ? { confirm_password: message } : fields);
      if (text) setNotice({ variant: "destructive", text });
    }
  };
  const error = (fields: Record<string, string[]>, name: string, id: string) =>
    fields[name] && <p id={id} role="alert" className="text-xs text-destructive">{fields[name]!.join(" ")}</p>;

  return (
    <Page>
      <PageHeader title="Your account" description="Manage your profile, password, and signed-in sessions."
        eyebrow="Settings" />
      <PausedBanner scope={ACCOUNT} />
      {notice && <Alert variant={notice.variant}>{notice.text}</Alert>}
      <div className="grid min-w-0 items-start gap-6 lg:grid-cols-[minmax(0,1fr)_minmax(0,2fr)]">
        <Card className="min-w-0">
          <CardHeader>
            <CardTitle>Profile</CardTitle>
            <p className="text-sm text-muted-foreground">Your signed-in identity.</p>
          </CardHeader>
          <CardContent>
            {panel.data && (
              <dl className="flex flex-col gap-5 text-sm">
                <div className="min-w-0">
                  <dt className="text-muted-foreground">Name</dt>
                  <dd className="mt-1 break-words font-medium">{panel.data.user.name}</dd>
                </div>
                <div className="min-w-0">
                  <dt className="text-muted-foreground">Email</dt>
                  <dd className="mt-1 break-all font-medium">{panel.data.user.email}</dd>
                </div>
              </dl>
            )}
          </CardContent>
        </Card>
        <section aria-label="Security" className="flex min-w-0 flex-col gap-6">
          <Card>
            <CardHeader>
              <CardTitle>Change password</CardTitle>
              <p id="password-help" className="text-sm text-muted-foreground">Choose a new password. Your
                other sessions will be signed out.</p>
            </CardHeader>
            <CardContent>
              <form className="flex max-w-lg flex-col gap-5" onSubmit={(e) => void submitPassword(e)}
                noValidate aria-describedby="password-help">
                <div className="flex flex-col gap-2">
                  <Label htmlFor="current-password">Current password</Label>
                  <Input id="current-password" type="password" autoComplete="current-password"
                    value={pw.current_password}
                    onChange={(e) => setPw({ ...pw, current_password: e.target.value })}
                    disabled={change.isPending} aria-invalid={!!pwFields.current_password}
                    aria-describedby={pwFields.current_password ? "current-password-error" : undefined} />
                  {error(pwFields, "current_password", "current-password-error")}
                </div>
                <div className="flex flex-col gap-2">
                  <Label htmlFor="new-password">New password</Label>
                  <Input id="new-password" type="password" autoComplete="new-password"
                    value={pw.password}
                    onChange={(e) => setPw({ ...pw, password: e.target.value })}
                    disabled={change.isPending} aria-invalid={!!pwFields.password}
                    aria-describedby={pwFields.password ? "new-password-error" : undefined} />
                  {error(pwFields, "password", "new-password-error")}
                </div>
                <div className="flex flex-col gap-2">
                  <Label htmlFor="confirm-new-password">Confirm new password</Label>
                  <Input id="confirm-new-password" type="password" autoComplete="new-password"
                    value={pw.password_confirmation}
                    onChange={(e) => setPw({ ...pw, password_confirmation: e.target.value })}
                    disabled={change.isPending} aria-invalid={!!pwFields.password_confirmation}
                    aria-describedby={pwFields.password_confirmation ? "confirm-new-password-error" : undefined} />
                  {error(pwFields, "password_confirmation", "confirm-new-password-error")}
                </div>
                <div className="border-t border-border pt-5">
                  <Button type="submit" disabled={paused || change.isPending}>
                    Change password
                  </Button>
                </div>
              </form>
            </CardContent>
          </Card>
          <Card>
            <CardHeader>
              <CardTitle>Other sessions</CardTitle>
            </CardHeader>
            <CardContent>
              <form className="flex max-w-lg flex-col gap-5" onSubmit={(e) => void submitOthers(e)}
                noValidate aria-describedby="sessions-help">
                <p id="sessions-help" className="text-sm leading-6 text-muted-foreground">Signs you out
                  on every other browser and revokes your API tokens.</p>
                <div className="flex flex-col gap-2">
                  <Label htmlFor="confirm-password">Password</Label>
                  <Input id="confirm-password" type="password" autoComplete="current-password"
                    value={confirmPw} onChange={(e) => setConfirmPw(e.target.value)}
                    disabled={others.isPending} aria-invalid={!!otherFields.confirm_password}
                    aria-describedby={otherFields.confirm_password ? "sessions-help confirm-password-error" : "sessions-help"} />
                  {error(otherFields, "confirm_password", "confirm-password-error")}
                </div>
                <div className="border-t border-border pt-5">
                  <Button type="submit" variant="outline" disabled={paused || !confirmPw || others.isPending}>
                    Sign out other sessions
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
