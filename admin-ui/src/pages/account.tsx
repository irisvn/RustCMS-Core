import { type FormEvent, useState } from "react";
import { ApiError } from "@/api/client";
import { useChangePassword, useLogoutOthers } from "@/api/system";
import { usePanel } from "@/api/queries";
import { Alert } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Input, Label } from "@/components/ui/input";

type Notice = { variant: "default" | "destructive"; text: string } | null;

function errorsOf(e: unknown): { fields: Record<string, string[]>; text: string | null } {
  if (e instanceof ApiError && e.status === 422) return { fields: e.fieldErrors, text: null };
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
  const error = (fields: Record<string, string[]>, name: string) =>
    fields[name] && <p role="alert" className="text-xs text-destructive">{fields[name]!.join(" ")}</p>;

  return (
    <div className="flex max-w-xl flex-col gap-4">
      <h1 className="text-xl font-semibold">Your account</h1>
      {panel.data && (
        <p className="text-sm text-muted-foreground">
          Signed in as {panel.data.user.name} ({panel.data.user.email}).
        </p>
      )}
      {notice && <Alert variant={notice.variant}>{notice.text}</Alert>}
      <Card>
        <CardHeader>
          <CardTitle>Change password</CardTitle>
        </CardHeader>
        <CardContent>
          <form className="flex flex-col gap-4" onSubmit={(e) => void submitPassword(e)} noValidate>
            <div className="flex flex-col gap-1.5">
              <Label htmlFor="current-password">Current password</Label>
              <Input id="current-password" type="password" autoComplete="current-password" value={pw.current_password} onChange={(e) => setPw({ ...pw, current_password: e.target.value })} disabled={change.isPending} />
              {error(pwFields, "current_password")}
            </div>
            <div className="flex flex-col gap-1.5">
              <Label htmlFor="new-password">New password</Label>
              <Input id="new-password" type="password" autoComplete="new-password" value={pw.password} onChange={(e) => setPw({ ...pw, password: e.target.value })} disabled={change.isPending} />
              {error(pwFields, "password")}
            </div>
            <div className="flex flex-col gap-1.5">
              <Label htmlFor="confirm-new-password">Confirm new password</Label>
              <Input id="confirm-new-password" type="password" autoComplete="new-password" value={pw.password_confirmation} onChange={(e) => setPw({ ...pw, password_confirmation: e.target.value })} disabled={change.isPending} />
              {error(pwFields, "password_confirmation")}
            </div>
            <div>
              <Button type="submit" disabled={change.isPending}>
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
          <form className="flex flex-col gap-4" onSubmit={(e) => void submitOthers(e)} noValidate>
            <p className="text-sm text-muted-foreground">Signs you out on every other browser and revokes your API tokens.</p>
            <div className="flex flex-col gap-1.5">
              <Label htmlFor="confirm-password">Password</Label>
              <Input id="confirm-password" type="password" autoComplete="current-password" value={confirmPw} onChange={(e) => setConfirmPw(e.target.value)} disabled={others.isPending} />
              {error(otherFields, "confirm_password")}
            </div>
            <div>
              <Button type="submit" variant="outline" disabled={!confirmPw || others.isPending}>
                Sign out other sessions
              </Button>
            </div>
          </form>
        </CardContent>
      </Card>
    </div>
  );
}
