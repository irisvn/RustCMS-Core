import { useNavigate } from "@tanstack/react-router";
import { type FormEvent, useState } from "react";
import { ArrowRight, PanelsTopLeft } from "lucide-react";
import { ApiError } from "@/api/client";
import { useSession } from "@/api/context";
import type { Me } from "@/api/types";
import { Alert } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Input, Label } from "@/components/ui/input";
import { LanguageSelect } from "@/components/language";
import { useT } from "@/i18n";

/** The product name (not translated). */
const PRODUCT = "rustcms";

export function LoginPage({ redirect }: { redirect?: string }) {
  const t = useT();
  const session = useSession();
  const navigate = useNavigate();
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  // The failure itself: its messages are read in the current language.
  const [failed, setFailed] = useState<unknown>(null);
  const errors: Record<string, string[]> = failed instanceof ApiError && failed.status === 422 ? failed.fieldErrors : {};
  const message = failed === null || (failed instanceof ApiError && failed.status === 422)
    ? null
    : failed instanceof Error ? failed.message : t("login.failed");
  const [pending, setPending] = useState(false);

  const submit = async (e: FormEvent) => {
    e.preventDefault();
    setFailed(null);
    setPending(true);
    try {
      // The CSRF token of the (anonymous) session; login rotates both.
      await session.api.post<Me>("/auth/login", { email, password });
      session.signedIn();
      await session.api.csrfToken();
      await navigate({ to: session.afterLogin(redirect), replace: true });
    } catch (err) {
      setFailed(err);
    } finally {
      setPending(false);
    }
  };

  return (
    <main className="flex min-h-dvh flex-col items-center justify-center gap-8 bg-background p-4 sm:p-8">
      <div className="flex items-center gap-2.5 text-base font-semibold tracking-tight"><span className="flex size-9 items-center justify-center rounded-lg bg-primary text-primary-foreground"><PanelsTopLeft className="size-5" /></span>{PRODUCT}</div>
      <Card className="w-full max-w-sm">
        <CardHeader className="gap-2 p-6 pb-0 text-center">
          <CardTitle className="text-2xl tracking-tight">{t("login.title")}</CardTitle>
          <p className="text-sm text-muted-foreground">{t("login.subtitle")}</p>
        </CardHeader>
        <CardContent className="p-6 pt-6">
          <form className="flex flex-col gap-4" onSubmit={(e) => void submit(e)} noValidate>
            {message && <Alert variant="destructive">{message}</Alert>}
            <div className="flex flex-col gap-1.5">
              <Label htmlFor="email">{t("field.email")}</Label>
              <Input
                id="email"
                type="email"
                autoComplete="username"
                required
                value={email}
                onChange={(e) => setEmail(e.target.value)}
                aria-invalid={!!errors.email || undefined}
                aria-describedby={errors.email ? "email-error" : undefined}
              />
              {errors.email && <p id="email-error" role="alert" className="text-xs text-destructive">{errors.email.join(" ")}</p>}
            </div>
            <div className="flex flex-col gap-1.5">
              <Label htmlFor="password">{t("field.password")}</Label>
              <Input
                id="password"
                type="password"
                autoComplete="current-password"
                required
                value={password}
                onChange={(e) => setPassword(e.target.value)}
                aria-invalid={!!errors.password || undefined}
                aria-describedby={errors.password ? "password-error" : undefined}
              />
              {errors.password && <p id="password-error" role="alert" className="text-xs text-destructive">{errors.password.join(" ")}</p>}
            </div>
            <Button type="submit" disabled={pending}>
              {pending ? t("login.pending") : <>{t("login.submit")} <ArrowRight className="ml-auto" /></>}
            </Button>
          </form>
        </CardContent>
      </Card>
      <div className="flex flex-col items-center gap-3">
        <p className="text-center text-xs text-muted-foreground">{t("login.tagline")}</p>
        <LanguageSelect />
      </div>
    </main>
  );
}
