// System users: list, create, edit (roles, status, password).
import { Link, useNavigate } from "@tanstack/react-router";
import { ShieldCheck } from "lucide-react";
import { useQueryClient } from "@tanstack/react-query";
import { type FormEvent, useEffect, useState } from "react";
import { ApiError } from "@/api/client";
import { usePanel } from "@/api/queries";
import { USERS, useCreateUser, useDeleteUser, useResetPassword, useRoles, useSetActive, useSetUserRoles, useUpdateProfile, useUser, useUsers } from "@/api/system";
import type { SystemRole } from "@/api/types";
import { Page, PageHeader } from "@/components/layout";
import { ErrorState, Loading, PausedBanner } from "@/components/states";
import { Alert } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Checkbox, Switch } from "@/components/ui/checkbox";
import { Dialog } from "@/components/ui/dialog";
import { Input, Label } from "@/components/ui/input";

import { Field, type Errors, type Failure, failure, linkButton, useMounted } from "./shared";
import { formatCount, later, show, type Text, useT } from "@/i18n";

export function UsersPage() {
  const t = useT();
  const panel = usePanel();
  const [search, setSearch] = useState("");
  const [debounced, setDebounced] = useState("");
  const [page, setPage] = useState(1);
  useEffect(() => {
    if (search === debounced) return;
    const timer = setTimeout(() => {
      setDebounced(search);
      setPage(1);
    }, 300);
    return () => clearTimeout(timer);
  }, [search, debounced]);
  const users = useUsers(debounced, page);
  const lastPage = users.data?.meta.lastPage;
  // Deletions can shrink the list below the current page.
  useEffect(() => {
    if (lastPage !== undefined && page > lastPage) setPage(lastPage);
  }, [page, lastPage]);
  if (users.isLoading) return <Loading />;
  // A failed refresh or a refused search keeps the page (and the search box) usable.
  if (!users.data && !(users.error instanceof ApiError && users.error.status === 422)) return <ErrorState
    error={users.error} />;
  const rows = users.data?.data ?? [];
  const meta = users.data?.meta ?? { page: 1, perPage: 25, total: 0, lastPage: 1 };
  return (
    <Page>
      <PageHeader title={t("nav.users")} description={t("users.description")}
        eyebrow={t("nav.system")} actions={panel.data?.system?.createUsers && (
          <Link to="/system/users/create" className={linkButton}>
            {t("users.new")}
          </Link>
        )} />
      <PausedBanner scope={USERS} />
      <Card className="min-w-0 overflow-hidden">
        <div className="flex flex-col gap-2 border-b border-border p-4 sm:p-5">
          <Label htmlFor="search-users">{t("users.search")}</Label>
          <Input id="search-users" aria-label={t("users.search")} placeholder={t("users.searchPlaceholder")}
            className="max-w-sm" maxLength={100} value={search}
            onChange={(e) => setSearch(e.target.value)}
            aria-invalid={users.error instanceof ApiError && users.error.status === 422}
            aria-describedby={users.error ? "search-users-error" : undefined} />
          {users.error && (
            <p id="search-users-error" role="alert" className="text-xs text-destructive">
              {users.error instanceof ApiError && users.error.status === 422
                ? (Object.values(users.error.fieldErrors)[0]?.[0] ?? users.error.message)
                : t("users.refreshFailed")}
            </p>
          )}
        </div>
        <div className="overflow-x-auto">
          <table aria-label={t("nav.users")} className="w-full min-w-[640px] text-sm">
            <thead className="border-b border-border bg-muted/40 text-left text-xs text-muted-foreground">
              <tr>
                <th scope="col" className="px-5 py-3 font-medium">{t("field.name")}</th>
                <th scope="col" className="px-5 py-3 font-medium">{t("field.email")}</th>
                <th scope="col" className="px-5 py-3 font-medium">{t("nav.roles")}</th>
                <th scope="col" className="px-5 py-3 font-medium">{t("users.status")}</th>
              </tr>
            </thead>
            <tbody>
              {rows.map((u) => (
                <tr key={u.id} className="border-b border-border last:border-0 hover:bg-muted/50">
                  <td className="px-5 py-4">
                    <Link to="/system/users/$id" params={{ id: u.id }}
                      className="font-medium text-primary hover:underline">
                      {u.name}
                    </Link>
                    {u.isSelf && <span className="ml-2 text-xs text-muted-foreground">{t("users.you")}</span>}
                  </td>
                  <td className="px-5 py-4 text-muted-foreground">{u.email}</td>
                  <td className="px-5 py-4">
                    <div className="flex flex-wrap gap-1">
                      {u.roles.map((r) => (
                        <Badge key={r.id}>
                          {r.name}
                        </Badge>
                      ))}
                    </div>
                  </td>
                  <td className="px-5 py-4">{u.isActive ?
                    <Badge className="border border-border bg-background">{t("users.active")}</Badge> :
                    <Badge>{t("users.disabled")}</Badge>}</td>
                </tr>
              ))}
              {rows.length === 0 && (
                <tr>
                  <td colSpan={4} className="p-8 text-center text-muted-foreground">
                    {t("users.empty")}
                  </td>
                </tr>
              )}
            </tbody>
          </table>
        </div>
        <div
          className="flex flex-wrap items-center justify-between gap-3 border-t border-border px-5 py-4 text-sm">
          <span className="text-muted-foreground">
            {t("users.page", { page: formatCount(meta.page), last: formatCount(meta.lastPage), users: t("users.count", { count: meta.total, n: formatCount(meta.total) }) })}
          </span>
          <div className="flex gap-2">
            <Button size="sm" variant="outline" disabled={page <= 1} onClick={() => setPage(page - 1)}>
              {t("list.previous")}
            </Button>
            <Button size="sm" variant="outline" disabled={page >= meta.lastPage}
              onClick={() => setPage(page + 1)}>
              {t("list.next")}
            </Button>
          </div>
        </div>
      </Card>
    </Page>
  );
}

function RolePicker({
  roles,
  selected,
  onChange,
  disabled,
  describedBy,
}: {
  roles: SystemRole[];
  selected: string[];
  onChange: (ids: string[]) => void;
  disabled?: boolean;
  describedBy?: string;
}) {
  const t = useT();
  return (
    <fieldset className="flex min-w-0 flex-col gap-2" aria-describedby={describedBy}>
      <legend className="mb-3 text-sm font-medium">{t("nav.roles")}</legend>
      {roles.map((r) => (
        <label key={r.id}
          className="flex min-w-0 items-center gap-3 rounded-md border border-border px-3 py-3 text-sm transition-colors hover:bg-muted/40 has-disabled:cursor-not-allowed has-disabled:opacity-60">
          <Checkbox
            className="shrink-0"
            aria-describedby={describedBy}
            checked={selected.includes(r.id)}
            disabled={disabled || !r.abilities.assign}
            onChange={(e) => onChange(e.target.checked ? [...selected, r.id] : selected.filter((id) => id !== r.id))}
          />
          <span className="min-w-0 break-all font-medium">{r.name}</span>
          {r.isSystem && <ShieldCheck className="ml-auto size-4 shrink-0 text-muted-foreground" role="img"
            aria-label={t("users.systemRole")} />}
        </label>
      ))}
    </fieldset>
  );
}

export function UserCreatePage() {
  const t = useT();
  const navigate = useNavigate();
  const mounted = useMounted();
  const create = useCreateUser();
  const roles = useRoles();
  const [form, setForm] = useState({ name: "", email: "", password: "" });
  const [roleIds, setRoleIds] = useState<string[]>([]);
  const [created, setCreated] = useState<string | null>(null);
  const [errors, setErrors] = useState<{ fields: Errors; form: string | null; stale?: boolean }>({
    fields: {},
    form: null
  });
  const submit = async (e: FormEvent) => {
    e.preventDefault();
    setErrors({ fields: {}, form: null, stale: false });
    setCreated(null);
    try {
      const user = await create.mutateAsync({ ...form, roleIds });
      // Left the page meanwhile: do not take over wherever the user is now.
      if (!mounted.current) return;
      if (user) {
        await navigate({ to: "/system/users/$id", params: { id: user.id } });
      } else {
        // Created, but this user may not view users: stay here, ready for the next one.
        setCreated(form.email);
        setForm({ name: "", email: "", password: "" });
        setRoleIds([]);
      }
    } catch (err) {
      if (mounted.current) setErrors(failure(err));
    }
  };
  return (
    <Page>
      <PageHeader title={t("users.new")} description={t("users.newDescription")} eyebrow={t("nav.system")} />
      <PausedBanner scope={USERS} />
      <form className="flex w-full max-w-2xl flex-col gap-6" onSubmit={(e) => void submit(e)} noValidate>
        {created && <Alert>{t("users.created", { email: created })}</Alert>}
        {errors.form && <Alert variant="destructive">{errors.form}</Alert>}
        <Card>
          <CardHeader>
            <CardTitle>{t("users.details")}</CardTitle>
            <p className="text-sm text-muted-foreground">{t("users.detailsText")}</p>
          </CardHeader>
          <CardContent className="flex flex-col gap-5">
            <Field id="name" label={t("field.name")} errors={errors.fields.name}>
              <Input id="name" value={form.name}
                onChange={(e) => setForm({ ...form, name: e.target.value })}
                disabled={create.isPending} aria-invalid={!!errors.fields.name}
                aria-describedby={errors.fields.name ? "name-error" : undefined} />
            </Field>
            <Field id="email" label={t("field.email")} errors={errors.fields.email}>
              <Input id="email" type="email" value={form.email}
                onChange={(e) => setForm({ ...form, email: e.target.value })}
                disabled={create.isPending} aria-invalid={!!errors.fields.email}
                aria-describedby={errors.fields.email ? "email-error" : undefined} />
            </Field>
            <Field id="password" label={t("field.password")} errors={errors.fields.password}>
              <Input id="password" type="password" autoComplete="new-password" value={form.password}
                onChange={(e) => setForm({ ...form, password: e.target.value })}
                disabled={create.isPending} aria-invalid={!!errors.fields.password}
                aria-describedby={errors.fields.password ? "password-error" : undefined} />
            </Field>
            {roles.data && roles.data.some((r) => r.abilities.assign) && (
              <div className="flex flex-col gap-2 border-t border-border pt-5">
                <RolePicker roles={roles.data} selected={roleIds} onChange={setRoleIds}
                  disabled={create.isPending}
                  describedBy={errors.fields.roleIds ? "roles-error" : undefined} />
                {errors.fields.roleIds && <p id="roles-error" role="alert"
                  className="text-xs text-destructive">{errors.fields.roleIds.join(" ")}</p>}
              </div>
            )}
          </CardContent>
        </Card>
        <div className="flex flex-wrap gap-3">
          <Button type="submit" disabled={create.isPending}>
            {create.isPending ? t("users.creating") : t("users.create")}
          </Button>
          <Link to="/system/users"
            className="inline-flex h-10 items-center justify-center rounded-md border border-border px-4 text-sm font-medium transition-colors hover:bg-muted focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring">
            {t("action.cancel")}
          </Link>
        </div>
      </form>
    </Page>
  );
}

export function UserPage({ id }: { id: string }) {
  const t = useT();
  const navigate = useNavigate();
  const client = useQueryClient();
  const mounted = useMounted();
  const user = useUser(id);
  const roles = useRoles(user.data?.abilities.assignRoles === true);
  const profile = useUpdateProfile(id);
  const setRoles = useSetUserRoles(id);
  const setActive = useSetActive(id);
  const reset = useResetPassword(id);
  const remove = useDeleteUser();
  const [draft, setDraft] = useState<{ name: string; email: string } | null>(null);
  const [profileBase, setProfileBase] = useState<{ name: string; email: string }>({ name: "", email: "" });
  // Changing one's own sign-in email asks for the current password.
  const [currentPassword, setCurrentPassword] = useState("");
  const [stale, setStale] = useState(false);
  const [roleDraft, setRoleDraft] = useState<string[] | null>(null);
  // What the draft started from, fixed when editing begins (a refetch must not move it).
  const [roleBase, setRoleBase] = useState<string[]>([]);
  const [password, setPassword] = useState("");
  const [notice, setNotice] = useState<{ variant: "default" | "destructive"; text: Text } | null>(null);
  // The last failure: its field messages are read in the current language.
  const [failed, setFailed] = useState<Failure | null>(null);
  const errors: Errors = failed?.fields ?? {};
  const [confirm, setConfirm] = useState(false);

  if (user.isLoading) return <Loading />;
  if (!user.data) return <ErrorState error={user.error} />;
  const u = user.data;
  const values = draft ?? { name: u.name, email: u.email };
  const ownEmailChange = u.isSelf && values.email.trim().toLowerCase() !== u.email.toLowerCase();
  const selectedRoles = roleDraft ?? u.roles.map((r) => r.id);
  const shownFields = ["name", "email", "password", "current_password", "roleIds"];
  const discard = () => {
    setDraft(null);
    setRoleDraft(null);
    setStale(false);
    setNotice(null);
  };
  const run = async (action: () => Promise<unknown>, done: Text) => {
    setNotice(null);
    setFailed(null);
    try {
      await action();
      if (mounted.current) setNotice({ variant: "default", text: done });
      return mounted.current;
    } catch (e) {
      if (!mounted.current) return false;
      const f = failure(e);
      setFailed(f);
      // Errors without an input of their own still reach the user.
      const text = () => {
        const other = Object.entries(f.fields).filter(([k]) => !shownFields.includes(k)).flatMap(([, m]) => m);
        return [f.form, ...other].filter(Boolean).join(" ") || (Object.keys(f.fields).length ? "" : t("system.notSaved"));
      };
      if (text()) setNotice({ variant: "destructive", text });
      if (f.stale) {
        // Someone changed it meanwhile: keep the draft (and what it started from),
        // refresh the current values, and let the user discard explicitly.
        setStale(true);
        void client.invalidateQueries({ queryKey: ["system"] });
      }
      return false;
    }
  };

  return (
    <Page>
      <PageHeader title={u.name} description={t("users.userDescription")}
        eyebrow={t("users.eyebrow")} actions={u.abilities.delete && (
          <Button variant="destructive" onClick={() => setConfirm(true)} disabled={remove.isPending}>
            {t("users.delete")}
          </Button>
        )} />
      <PausedBanner scope={USERS} />
      {u.superAdmin && <div><Badge>{t("users.superAdmin")}</Badge></div>}
      {u.isSelf && (
        <Alert>
          {t("users.selfBefore")}{" "}
          <Link to="/account" className="underline">
            {t("users.selfLink")}
          </Link>
          {t("users.selfAfter")}
        </Alert>
      )}
      {notice && <Alert variant={notice.variant}>{show(notice.text)}</Alert>}
      {stale && (
        <Alert variant="warning">
          {t("users.stale")}{" "}
          <button type="button" className="underline" onClick={discard}>
            {t("system.discard")}
          </button>
        </Alert>
      )}

      <div className="grid min-w-0 items-start gap-6 lg:grid-cols-[minmax(0,1.25fr)_minmax(0,1fr)]">
        <Card className="min-w-0">
          <CardHeader>
            <CardTitle>{t("account.profile")}</CardTitle>
            <p className="text-sm text-muted-foreground">{t("users.profileText")}</p>
          </CardHeader>
          <CardContent>
            <form
              className="flex max-w-lg flex-col gap-5"
              noValidate
              onSubmit={(e) => {
                e.preventDefault();
                void run(
                  () =>
                    profile.mutateAsync({
                      ...values,
                      expected: profileBase,
                      currentPassword: ownEmailChange ? currentPassword : undefined,
                    }),
                  later("users.profileSaved"),
                ).then((ok) => {
                  if (ok) {
                    setDraft(null);
                    setCurrentPassword("");
                  }
                });
              }}
            >
              <Field id="name" label={t("field.name")} errors={errors.name}>
                <Input id="name" value={values.name} disabled={!u.abilities.update || profile.isPending}
                  aria-invalid={!!errors.name}
                  aria-describedby={errors.name ? "name-error" : undefined} onChange={(e) => {
                    if (!draft) setProfileBase({ name: u.name, email: u.email });
                    setDraft({ ...values, name: e.target.value });
                  }} />
              </Field>
              <Field id="email" label={t("field.email")} errors={errors.email}>
                <Input id="email" type="email" value={values.email}
                  disabled={!u.abilities.changeEmail || profile.isPending}
                  aria-invalid={!!errors.email}
                  aria-describedby={errors.email ? "email-error" : undefined} onChange={(e) => {
                    if (!draft) setProfileBase({ name: u.name, email: u.email });
                    setDraft({ ...values, email: e.target.value });
                  }} />
              </Field>
              {ownEmailChange && (
                <Field id="current-password" label={t("users.currentPasswordForEmail")}
                  errors={errors.current_password}>
                  <Input id="current-password" type="password" autoComplete="current-password"
                    value={currentPassword} onChange={(e) => setCurrentPassword(e.target.value)}
                    disabled={profile.isPending} aria-invalid={!!errors.current_password}
                    aria-describedby={errors.current_password ? "current-password-error" : undefined} />
                </Field>
              )}
              {u.abilities.update && (
                <div className="border-t border-border pt-5">
                  <Button type="submit" disabled={!draft || profile.isPending}>
                    {t("users.saveProfile")}
                  </Button>
                </div>
              )}
            </form>
          </CardContent>
        </Card>

        <div className="flex min-w-0 flex-col gap-6">
          {u.abilities.assignRoles && roles.data && (
            <Card>
              <CardHeader>
                <CardTitle>{t("nav.roles")}</CardTitle>
                <p className="text-sm text-muted-foreground">{t("users.rolesText")}</p>
              </CardHeader>
              <CardContent className="flex flex-col gap-5">
                <RolePicker
                  roles={roles.data}
                  selected={selectedRoles}
                  onChange={(ids) => {
                    if (roleDraft === null) setRoleBase(u.roles.map((r) => r.id));
                    setRoleDraft(ids);
                  }}
                  disabled={setRoles.isPending}
                  describedBy={errors.roleIds ? "roles-error" : undefined}
                />
                {errors.roleIds && <p id="roles-error" role="alert"
                  className="text-xs text-destructive">{errors.roleIds.join(" ")}</p>}
                <div className="border-t border-border pt-5">
                  <Button
                    disabled={!roleDraft || setRoles.isPending}
                    onClick={() => void run(() => setRoles.mutateAsync({
                      roleIds: selectedRoles,
                      expected: roleBase
                    }), later("users.rolesSaved")).then((ok) => ok && setRoleDraft(null))}
                  >
                    {t("users.saveRoles")}
                  </Button>
                </div>
              </CardContent>
            </Card>
          )}

          {(u.abilities.activate || u.abilities.resetPassword) && (
            <Card>
              <CardHeader>
                <CardTitle>{t("users.access")}</CardTitle>
                <p className="text-sm text-muted-foreground">{t("users.accessText")}</p>
              </CardHeader>
              <CardContent className="flex flex-col gap-5">
                {u.abilities.activate && (
                  <label
                    className="flex flex-wrap items-center gap-3 rounded-md border border-border p-3 text-sm">
                    <Switch
                      aria-label={t("users.maySignIn")}
                      checked={u.isActive}
                      disabled={setActive.isPending}
                      onCheckedChange={(active) =>
                        void run(
                          () => setActive.mutateAsync({ active, expected: u.isActive }),
                          later(active ? "users.enabled" : "users.disabledAll"),
                        )
                      }
                    />
                    {t("users.maySignIn")}
                  </label>
                )}
                {u.abilities.resetPassword && (
                  <form
                    className="flex max-w-lg flex-col items-start gap-4"
                    noValidate
                    onSubmit={(e) => {
                      e.preventDefault();
                      void run(() => reset.mutateAsync(password), later("users.passwordSet")).then(
                        (ok) => ok && setPassword(""),
                      );
                    }}
                  >
                    <div className="w-full">
                      <Field id="new-password" label={t("account.newPassword")} errors={errors.password}>
                        <Input id="new-password" type="password" autoComplete="new-password"
                          value={password} onChange={(e) => setPassword(e.target.value)}
                          disabled={reset.isPending} aria-invalid={!!errors.password}
                          aria-describedby={errors.password ? "reset-password-help new-password-error" : "reset-password-help"} />
                      </Field>
                      <p id="reset-password-help"
                        className="mt-2 text-xs leading-5 text-muted-foreground">{t("users.resetHelp")}</p>
                    </div>
                    <Button type="submit" variant="outline" disabled={!password || reset.isPending}>
                      {t("users.setPassword")}
                    </Button>
                  </form>
                )}
              </CardContent>
            </Card>
          )}
        </div>
      </div>

      <Link to="/system/users"
        className="w-fit text-sm text-muted-foreground underline-offset-4 hover:text-foreground hover:underline focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring">
        {t("users.back")}
      </Link>
      <Dialog open={confirm} onClose={() => setConfirm(false)} title={t("users.confirmDelete")}>
        <p className="break-words text-sm text-muted-foreground">{t("users.confirmDeleteText", { email: u.email })}</p>
        <div className="flex flex-wrap justify-end gap-2">
          <Button variant="outline" onClick={() => setConfirm(false)}>
            {t("action.cancel")}
          </Button>
          <Button
            variant="destructive"
            onClick={() => {
              setConfirm(false);
              void run(() => remove.mutateAsync(id), later("list.deleted")).then((ok) => {
                if (ok && mounted.current) void navigate({ to: "/system/users" });
              });
            }}
          >
            {t("action.delete")}
          </Button>
        </div>
      </Dialog>
    </Page>
  );
}

// ---------------------------------------------------------------------------
// Roles
