// Pages before sign-in: "Forgot password?", and choosing a password from a
// reset link or an invitation. The token arrives in the URL fragment (never
// sent to the server by the browser, nor in a `Referer`); it is read once and
// removed from the address bar.
import { Link, useNavigate } from "@tanstack/react-router";
import { type FormEvent, type ReactNode, useEffect, useState } from "react";
import { PanelsTopLeft } from "lucide-react";
import { ApiError } from "@/api/client";
import { useSession } from "@/api/context";
import { Alert } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Input, Label } from "@/components/ui/input";
import { LanguageSelect } from "@/components/language";
import { useT } from "@/i18n";

/** The product name (not translated). */
const PRODUCT = "rustcms";

function Frame({ title, subtitle, children }: { title: string; subtitle: string; children: ReactNode }) {
  return (
    <main className="flex min-h-dvh flex-col items-center justify-center gap-8 bg-background p-4 sm:p-8">
      <div className="flex items-center gap-2.5 text-base font-semibold tracking-tight"><span className="flex size-9 items-center justify-center rounded-lg bg-primary text-primary-foreground"><PanelsTopLeft className="size-5" /></span>{PRODUCT}</div>
      <Card className="w-full max-w-sm">
        <CardHeader className="gap-2 p-6 pb-0 text-center">
          <CardTitle className="text-2xl tracking-tight">{title}</CardTitle>
          <p className="text-sm text-muted-foreground">{subtitle}</p>
        </CardHeader>
        <CardContent className="p-6 pt-6">{children}</CardContent>
      </Card>
      <LanguageSelect />
    </main>
  );
}

function FieldError({ id, messages }: { id: string; messages?: string[] }) {
  if (!messages?.length) return null;
  return <p id={id} role="alert" className="text-xs text-destructive">{messages.join(" ")}</p>;
}

function errorsOf(failed: unknown): Record<string, string[]> {
  return failed instanceof ApiError && failed.status === 422 ? failed.fieldErrors : {};
}

/** A top-level message for failures other than field errors. */
function useMessage(failed: unknown): string | null {
  const t = useT();
  if (failed === null || (failed instanceof ApiError && failed.status === 422)) return null;
  return failed instanceof Error ? failed.message : t("login.failed");
}

export function ForgotPasswordPage() {
  const t = useT();
  const session = useSession();
  const [email, setEmail] = useState("");
  const [sent, setSent] = useState(false);
  const [pending, setPending] = useState(false);
  const [failed, setFailed] = useState<unknown>(null);
  const errors = errorsOf(failed);
  const message = useMessage(failed);
  const submit = async (e: FormEvent) => {
    e.preventDefault();
    setFailed(null);
    setPending(true);
    try {
      await session.api.post("/auth/forgot-password", { email });
      setSent(true);
    } catch (err) {
      setFailed(err);
    } finally {
      setPending(false);
    }
  };
  return (
    <Frame title={t("recovery.forgotTitle")} subtitle={t("recovery.forgotSubtitle")}>
      {sent ? (
        <div className="flex flex-col gap-4">
          {/* The same whether an account exists or not. */}
          <Alert>{t("recovery.sent")}</Alert>
          <Link to="/login" className="text-sm text-primary underline-offset-4 hover:underline">{t("recovery.backToLogin")}</Link>
        </div>
      ) : (
        <form className="flex flex-col gap-4" onSubmit={(e) => void submit(e)} noValidate>
          {message && <Alert variant="destructive">{message}</Alert>}
          <div className="flex flex-col gap-1.5">
            <Label htmlFor="email">{t("field.email")}</Label>
            <Input id="email" type="email" autoComplete="username" required value={email}
              onChange={(e) => setEmail(e.target.value)} aria-invalid={!!errors.email || undefined}
              aria-describedby={errors.email ? "email-error" : undefined} />
            <FieldError id="email-error" messages={errors.email} />
          </div>
          <Button type="submit" disabled={pending || email.trim() === ""}>
            {pending ? t("recovery.sending") : t("recovery.send")}
          </Button>
          <Link to="/login" className="text-center text-sm text-muted-foreground underline-offset-4 hover:underline">{t("recovery.backToLogin")}</Link>
        </form>
      )}
    </Frame>
  );
}

/** The token of the URL fragment, read once (then removed from the address bar). */
function useFragmentToken(): string | null {
  const [token] = useState(() => {
    const params = new URLSearchParams(window.location.hash.replace(/^#/, ""));
    return params.get("token");
  });
  useEffect(() => {
    if (window.location.hash) {
      window.history.replaceState(window.history.state, "", window.location.pathname + window.location.search);
    }
  }, []);
  return token;
}

export function TokenPasswordPage({ purpose }: { purpose: "reset" | "invitation" }) {
  const t = useT();
  const session = useSession();
  const navigate = useNavigate();
  const token = useFragmentToken();
  const [name, setName] = useState("");
  const [password, setPassword] = useState("");
  const [confirmation, setConfirmation] = useState("");
  const [pending, setPending] = useState(false);
  const [failed, setFailed] = useState<unknown>(null);
  const errors = errorsOf(failed);
  const message = useMessage(failed);
  const invitation = purpose === "invitation";
  const submit = async (e: FormEvent) => {
    e.preventDefault();
    setFailed(null);
    setPending(true);
    try {
      const body: Record<string, string> = { token: token ?? "", password, password_confirmation: confirmation };
      if (invitation && name.trim()) body.name = name.trim();
      await session.api.post(invitation ? "/auth/invitation" : "/auth/reset-password", body);
      await navigate({ to: "/login", search: { done: invitation ? "invitation" : "reset" }, replace: true });
    } catch (err) {
      setFailed(err);
    } finally {
      setPending(false);
    }
  };
  const title = invitation ? t("recovery.invitationTitle") : t("recovery.resetTitle");
  const subtitle = invitation ? t("recovery.invitationSubtitle") : t("recovery.resetSubtitle");
  if (!token || errors.token) {
    return (
      <Frame title={title} subtitle={subtitle}>
        <div className="flex flex-col gap-4">
          <Alert variant="destructive">{invitation ? t("recovery.invitationInvalid") : t("recovery.linkInvalid")}</Alert>
          {!invitation && (
            <Link to="/forgot-password" className="text-sm text-primary underline-offset-4 hover:underline">{t("recovery.askAgain")}</Link>
          )}
          <Link to="/login" className="text-sm text-muted-foreground underline-offset-4 hover:underline">{t("recovery.backToLogin")}</Link>
        </div>
      </Frame>
    );
  }
  return (
    <Frame title={title} subtitle={subtitle}>
      <form className="flex flex-col gap-4" onSubmit={(e) => void submit(e)} noValidate>
        {message && <Alert variant="destructive">{message}</Alert>}
        {invitation && (
          <div className="flex flex-col gap-1.5">
            <Label htmlFor="name">{t("field.name")}</Label>
            <Input id="name" autoComplete="name" value={name} onChange={(e) => setName(e.target.value)}
              aria-invalid={!!errors.name || undefined} aria-describedby={errors.name ? "name-error" : undefined} />
            <FieldError id="name-error" messages={errors.name} />
          </div>
        )}
        <div className="flex flex-col gap-1.5">
          <Label htmlFor="password">{t("account.newPassword")}</Label>
          <Input id="password" type="password" autoComplete="new-password" required value={password}
            onChange={(e) => setPassword(e.target.value)} aria-invalid={!!errors.password || undefined}
            aria-describedby={errors.password ? "password-error" : undefined} />
          <FieldError id="password-error" messages={errors.password} />
        </div>
        <div className="flex flex-col gap-1.5">
          <Label htmlFor="password-confirmation">{t("recovery.confirm")}</Label>
          <Input id="password-confirmation" type="password" autoComplete="new-password" required value={confirmation}
            onChange={(e) => setConfirmation(e.target.value)} aria-invalid={!!errors.password_confirmation || undefined}
            aria-describedby={errors.password_confirmation ? "confirmation-error" : undefined} />
          <FieldError id="confirmation-error" messages={errors.password_confirmation} />
        </div>
        <Button type="submit" disabled={pending || !password}>
          {pending ? t("recovery.saving") : invitation ? t("recovery.accept") : t("recovery.save")}
        </Button>
      </form>
    </Frame>
  );
}
