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

export function LoginPage({ redirect }: { redirect?: string }) {
  const session = useSession();
  const navigate = useNavigate();
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const [errors, setErrors] = useState<Record<string, string[]>>({});
  const [message, setMessage] = useState<string | null>(null);
  const [pending, setPending] = useState(false);

  const submit = async (e: FormEvent) => {
    e.preventDefault();
    setErrors({});
    setMessage(null);
    setPending(true);
    try {
      // The CSRF token of the (anonymous) session; login rotates both.
      await session.api.post<Me>("/auth/login", { email, password });
      session.signedIn();
      await session.api.csrfToken();
      await navigate({ to: session.afterLogin(redirect), replace: true });
    } catch (err) {
      if (err instanceof ApiError && err.status === 422) setErrors(err.fieldErrors);
      else setMessage(err instanceof Error ? err.message : "Sign-in failed.");
    } finally {
      setPending(false);
    }
  };

  return (
    <main className="flex min-h-dvh flex-col items-center justify-center gap-8 bg-background p-4 sm:p-8">
      <div className="flex items-center gap-2.5 text-base font-semibold tracking-tight"><span className="flex size-9 items-center justify-center rounded-lg bg-primary text-primary-foreground"><PanelsTopLeft className="size-5" /></span>rustcms</div>
      <Card className="w-full max-w-sm">
        <CardHeader className="gap-2 p-6 pb-0 text-center">
          <CardTitle className="text-2xl tracking-tight">Sign in</CardTitle>
          <p className="text-sm text-muted-foreground">Enter your details to access your workspace.</p>
        </CardHeader>
        <CardContent className="p-6 pt-6">
          <form className="flex flex-col gap-4" onSubmit={(e) => void submit(e)} noValidate>
            {message && <Alert variant="destructive">{message}</Alert>}
            <div className="flex flex-col gap-1.5">
              <Label htmlFor="email">Email</Label>
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
              <Label htmlFor="password">Password</Label>
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
              {pending ? "Signing in…" : <>Sign in <ArrowRight className="ml-auto" /></>}
            </Button>
          </form>
        </CardContent>
      </Card>
      <p className="text-center text-xs text-muted-foreground">Your content. Your workspace.</p>
    </main>
  );
}
