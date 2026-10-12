import { useNavigate } from "@tanstack/react-router";
import { type FormEvent, useState } from "react";
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
    <div className="flex min-h-screen items-center justify-center p-4">
      <Card className="w-full max-w-sm">
        <CardHeader>
          <CardTitle>Sign in</CardTitle>
        </CardHeader>
        <CardContent>
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
              />
              {errors.email && <p role="alert" className="text-xs text-destructive">{errors.email.join(" ")}</p>}
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
              />
              {errors.password && <p role="alert" className="text-xs text-destructive">{errors.password.join(" ")}</p>}
            </div>
            <Button type="submit" disabled={pending}>
              {pending ? "Signing in…" : "Sign in"}
            </Button>
          </form>
        </CardContent>
      </Card>
    </div>
  );
}
