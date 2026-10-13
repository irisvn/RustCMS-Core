// System pages: users and roles. Every change is checked again by the server
// (RbacService); `abilities` only decide what is offered.
import { Link, useNavigate } from "@tanstack/react-router";
import { ShieldCheck } from "lucide-react";
import { useQueryClient } from "@tanstack/react-query";
import { type FormEvent, useEffect, useMemo, useRef, useState } from "react";
import { ApiError } from "@/api/client";
import { usePanel } from "@/api/queries";
import {
  groupPermissions,
  useCreateRole,
  useCreateUser,
  useDeleteRole,
  useDeleteUser,
  usePermissions,
  useResetPassword,
  useRole,
  useRoles,
  useSetActive,
  useSetRolePermissions,
  useSetUserRoles,
  useUpdateProfile,
  useUser,
  useUsers,
} from "@/api/system";
import type { PermissionInfo, SystemRole } from "@/api/types";
import { Page, PageHeader } from "@/components/layout";
import { ErrorState, Loading } from "@/components/states";
import { Alert } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Checkbox, Switch } from "@/components/ui/checkbox";
import { Dialog } from "@/components/ui/dialog";
import { Input, Label } from "@/components/ui/input";

type Errors = Record<string, string[]>;

/** Field errors of a 422, otherwise one form-level message. */
function failure(e: unknown): { fields: Errors; form: string | null; stale: boolean } {
  if (e instanceof ApiError && e.status === 422) {
    const form = e.formErrors.join(" ");
    return { fields: e.fieldErrors, form: form || null, stale: form.includes("changed by someone else") };
  }
  if (e instanceof ApiError && e.status === 403) {
    return { fields: {}, form: "You are not allowed to do this.", stale: false };
  }
  return { fields: {}, form: e instanceof Error ? e.message : "Something went wrong.", stale: false };
}

/** Whether the component is still mounted (completion handlers check it). */
function useMounted() {
  const mounted = useRef(true);
  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
    };
  }, []);
  return mounted;
}

function Field({ id, label, errors, children }: {
  id: string;
  label: string;
  errors?: string[];
  children: React.ReactNode
}) {
  return (
    <div className="flex min-w-0 flex-col gap-2">
      <Label htmlFor={id}>{label}</Label>
      {children}
      {errors && <p id={`${id}-error`} role="alert" className="text-xs text-destructive">{errors.join(" ")}</p>}
    </div>
  );
}

const linkButton =
  "inline-flex h-10 shrink-0 items-center justify-center gap-2 rounded-md bg-primary px-4 text-sm font-medium text-primary-foreground shadow-xs transition-colors hover:bg-primary/90 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2";

// ---------------------------------------------------------------------------
// Users

export function UsersPage() {
  const panel = usePanel();
  const [search, setSearch] = useState("");
  const [debounced, setDebounced] = useState("");
  const [page, setPage] = useState(1);
  useEffect(() => {
    if (search === debounced) return;
    const t = setTimeout(() => {
      setDebounced(search);
      setPage(1);
    }, 300);
    return () => clearTimeout(t);
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
      <PageHeader title="Users" description="Manage people, roles, and access to your admin panel."
        eyebrow="System" actions={panel.data?.system?.createUsers && (
          <Link to="/system/users/create" className={linkButton}>
            New user
          </Link>
        )} />
      <Card className="min-w-0 overflow-hidden">
        <div className="flex flex-col gap-2 border-b border-border p-4 sm:p-5">
          <Label htmlFor="search-users">Search users</Label>
          <Input id="search-users" aria-label="Search users" placeholder="Search name or email…"
            className="max-w-sm" maxLength={100} value={search}
            onChange={(e) => setSearch(e.target.value)}
            aria-invalid={users.error instanceof ApiError && users.error.status === 422}
            aria-describedby={users.error ? "search-users-error" : undefined} />
          {users.error && (
            <p id="search-users-error" role="alert" className="text-xs text-destructive">
              {users.error instanceof ApiError && users.error.status === 422
                ? (Object.values(users.error.fieldErrors)[0]?.[0] ?? users.error.message)
                : "Could not refresh the list."}
            </p>
          )}
        </div>
        <div className="overflow-x-auto">
          <table aria-label="Users" className="w-full min-w-[640px] text-sm">
            <thead className="border-b border-border bg-muted/40 text-left text-xs text-muted-foreground">
              <tr>
                <th scope="col" className="px-5 py-3 font-medium">Name</th>
                <th scope="col" className="px-5 py-3 font-medium">Email</th>
                <th scope="col" className="px-5 py-3 font-medium">Roles</th>
                <th scope="col" className="px-5 py-3 font-medium">Status</th>
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
                    {u.isSelf && <span className="ml-2 text-xs text-muted-foreground">(you)</span>}
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
                    <Badge className="border border-border bg-background">active</Badge> :
                    <Badge>disabled</Badge>}</td>
                </tr>
              ))}
              {rows.length === 0 && (
                <tr>
                  <td colSpan={4} className="p-8 text-center text-muted-foreground">
                    No users found.
                  </td>
                </tr>
              )}
            </tbody>
          </table>
        </div>
        <div
          className="flex flex-wrap items-center justify-between gap-3 border-t border-border px-5 py-4 text-sm">
          <span className="text-muted-foreground">
            Page {meta.page} of {meta.lastPage} · {meta.total} users
          </span>
          <div className="flex gap-2">
            <Button size="sm" variant="outline" disabled={page <= 1} onClick={() => setPage(page - 1)}>
              Previous
            </Button>
            <Button size="sm" variant="outline" disabled={page >= meta.lastPage}
              onClick={() => setPage(page + 1)}>
              Next
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
  return (
    <fieldset className="flex min-w-0 flex-col gap-2" aria-describedby={describedBy}>
      <legend className="mb-3 text-sm font-medium">Roles</legend>
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
            aria-label="system role" />}
        </label>
      ))}
    </fieldset>
  );
}

export function UserCreatePage() {
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
      <PageHeader title="New user" description="Create a user and choose their initial access." eyebrow="System" />
      <form className="flex w-full max-w-2xl flex-col gap-6" onSubmit={(e) => void submit(e)} noValidate>
        {created && <Alert>{created} was created.</Alert>}
        {errors.form && <Alert variant="destructive">{errors.form}</Alert>}
        <Card>
          <CardHeader>
            <CardTitle>User details</CardTitle>
            <p className="text-sm text-muted-foreground">Set the profile and sign-in credentials for this
              user.</p>
          </CardHeader>
          <CardContent className="flex flex-col gap-5">
            <Field id="name" label="Name" errors={errors.fields.name}>
              <Input id="name" value={form.name}
                onChange={(e) => setForm({ ...form, name: e.target.value })}
                disabled={create.isPending} aria-invalid={!!errors.fields.name}
                aria-describedby={errors.fields.name ? "name-error" : undefined} />
            </Field>
            <Field id="email" label="Email" errors={errors.fields.email}>
              <Input id="email" type="email" value={form.email}
                onChange={(e) => setForm({ ...form, email: e.target.value })}
                disabled={create.isPending} aria-invalid={!!errors.fields.email}
                aria-describedby={errors.fields.email ? "email-error" : undefined} />
            </Field>
            <Field id="password" label="Password" errors={errors.fields.password}>
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
            {create.isPending ? "Creating…" : "Create user"}
          </Button>
          <Link to="/system/users"
            className="inline-flex h-10 items-center justify-center rounded-md border border-border px-4 text-sm font-medium transition-colors hover:bg-muted focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring">
            Cancel
          </Link>
        </div>
      </form>
    </Page>
  );
}

export function UserPage({ id }: { id: string }) {
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
  const [notice, setNotice] = useState<{ variant: "default" | "destructive"; text: string } | null>(null);
  const [errors, setErrors] = useState<Errors>({});
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
  const run = async (action: () => Promise<unknown>, done: string) => {
    setNotice(null);
    setErrors({});
    try {
      await action();
      if (mounted.current) setNotice({ variant: "default", text: done });
      return mounted.current;
    } catch (e) {
      if (!mounted.current) return false;
      const f = failure(e);
      setErrors(f.fields);
      // Errors without an input of their own still reach the user.
      const other = Object.entries(f.fields).filter(([k]) => !shownFields.includes(k)).flatMap(([, m]) => m);
      const text = [f.form, ...other].filter(Boolean).join(" ") || (Object.keys(f.fields).length ? null : "Not saved.");
      if (text) setNotice({ variant: "destructive", text });
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
      <PageHeader title={u.name} description="Manage this user's profile and sign-in access."
        eyebrow="System / Users" actions={u.abilities.delete && (
          <Button variant="destructive" onClick={() => setConfirm(true)} disabled={remove.isPending}>
            Delete user
          </Button>
        )} />
      {u.superAdmin && <div><Badge>super admin</Badge></div>}
      {u.isSelf && (
        <Alert>
          This is your account: change your password or sign out other sessions on{" "}
          <Link to="/account" className="underline">
            your account page
          </Link>
          .
        </Alert>
      )}
      {notice && <Alert variant={notice.variant}>{notice.text}</Alert>}
      {stale && (
        <Alert variant="warning">
          Someone changed this user meanwhile; your unsaved changes are kept.{" "}
          <button type="button" className="underline" onClick={discard}>
            Discard my changes
          </button>
        </Alert>
      )}

      <div className="grid min-w-0 items-start gap-6 lg:grid-cols-[minmax(0,1.25fr)_minmax(0,1fr)]">
        <Card className="min-w-0">
          <CardHeader>
            <CardTitle>Profile</CardTitle>
            <p className="text-sm text-muted-foreground">Personal details and sign-in email.</p>
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
                  "Profile saved.",
                ).then((ok) => {
                  if (ok) {
                    setDraft(null);
                    setCurrentPassword("");
                  }
                });
              }}
            >
              <Field id="name" label="Name" errors={errors.name}>
                <Input id="name" value={values.name} disabled={!u.abilities.update || profile.isPending}
                  aria-invalid={!!errors.name}
                  aria-describedby={errors.name ? "name-error" : undefined} onChange={(e) => {
                    if (!draft) setProfileBase({ name: u.name, email: u.email });
                    setDraft({ ...values, name: e.target.value });
                  }} />
              </Field>
              <Field id="email" label="Email" errors={errors.email}>
                <Input id="email" type="email" value={values.email}
                  disabled={!u.abilities.changeEmail || profile.isPending}
                  aria-invalid={!!errors.email}
                  aria-describedby={errors.email ? "email-error" : undefined} onChange={(e) => {
                    if (!draft) setProfileBase({ name: u.name, email: u.email });
                    setDraft({ ...values, email: e.target.value });
                  }} />
              </Field>
              {ownEmailChange && (
                <Field id="current-password" label="Current password (to change your email)"
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
                    Save profile
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
                <CardTitle>Roles</CardTitle>
                <p className="text-sm text-muted-foreground">Choose which roles this user holds.</p>
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
                    }), "Roles saved.").then((ok) => ok && setRoleDraft(null))}
                  >
                    Save roles
                  </Button>
                </div>
              </CardContent>
            </Card>
          )}

          {(u.abilities.activate || u.abilities.resetPassword) && (
            <Card>
              <CardHeader>
                <CardTitle>Access</CardTitle>
                <p className="text-sm text-muted-foreground">Control sign-in and reset credentials.</p>
              </CardHeader>
              <CardContent className="flex flex-col gap-5">
                {u.abilities.activate && (
                  <label
                    className="flex flex-wrap items-center gap-3 rounded-md border border-border p-3 text-sm">
                    <Switch
                      aria-label="May sign in"
                      checked={u.isActive}
                      disabled={setActive.isPending}
                      onCheckedChange={(active) =>
                        void run(
                          () => setActive.mutateAsync({ active, expected: u.isActive }),
                          active ? "User enabled." : "User disabled and signed out everywhere.",
                        )
                      }
                    />
                    May sign in
                  </label>
                )}
                {u.abilities.resetPassword && (
                  <form
                    className="flex max-w-lg flex-col items-start gap-4"
                    noValidate
                    onSubmit={(e) => {
                      e.preventDefault();
                      void run(() => reset.mutateAsync(password), "Password set; the user was signed out everywhere.").then(
                        (ok) => ok && setPassword(""),
                      );
                    }}
                  >
                    <div className="w-full">
                      <Field id="new-password" label="New password" errors={errors.password}>
                        <Input id="new-password" type="password" autoComplete="new-password"
                          value={password} onChange={(e) => setPassword(e.target.value)}
                          disabled={reset.isPending} aria-invalid={!!errors.password}
                          aria-describedby={errors.password ? "reset-password-help new-password-error" : "reset-password-help"} />
                      </Field>
                      <p id="reset-password-help"
                        className="mt-2 text-xs leading-5 text-muted-foreground">Setting a
                        password signs this user out everywhere.</p>
                    </div>
                    <Button type="submit" variant="outline" disabled={!password || reset.isPending}>
                      Set password
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
        ← All users
      </Link>
      <Dialog open={confirm} onClose={() => setConfirm(false)} title="Delete user?">
        <p className="break-words text-sm text-muted-foreground">{u.email} will be deleted with their sessions
          and tokens.</p>
        <div className="flex flex-wrap justify-end gap-2">
          <Button variant="outline" onClick={() => setConfirm(false)}>
            Cancel
          </Button>
          <Button
            variant="destructive"
            onClick={() => {
              setConfirm(false);
              void run(() => remove.mutateAsync(id), "Deleted.").then((ok) => {
                if (ok && mounted.current) void navigate({ to: "/system/users" });
              });
            }}
          >
            Delete
          </Button>
        </div>
      </Dialog>
    </Page>
  );
}

// ---------------------------------------------------------------------------
// Roles

export function RolesPage() {
  const panel = usePanel();
  const roles = useRoles();
  if (roles.isLoading) return <Loading />;
  if (!roles.data) return <ErrorState error={roles.error} />;
  return (
    <Page>
      <PageHeader title="Roles" description="Organize access with roles and explicit permissions."
        eyebrow="System" actions={panel.data?.system?.createRoles && (
          <Link to="/system/roles/create" className={linkButton}>
            New role
          </Link>
        )} />
      <Card className="min-w-0 overflow-hidden">
        <div className="overflow-x-auto">
          <table aria-label="Roles" className="w-full min-w-[480px] text-sm">
            <thead className="border-b border-border bg-muted/40 text-left text-xs text-muted-foreground">
              <tr>
                <th scope="col" className="px-5 py-3 font-medium">Name</th>
                <th scope="col" className="px-5 py-3 font-medium">Permissions</th>
                <th scope="col" className="px-5 py-3 font-medium">Users</th>
              </tr>
            </thead>
            <tbody>
              {roles.data.map((r) => (
                <tr key={r.id} className="border-b border-border last:border-0 hover:bg-muted/50">
                  <td className="px-5 py-4">
                    <div className="flex flex-wrap items-center gap-2">
                      <Link to="/system/roles/$id" params={{ id: r.id }}
                        className="font-medium text-primary hover:underline">
                        {r.name}
                      </Link>
                      {r.isSystem && <Badge>system</Badge>} {r.isPrivileged &&
                        <Badge className="border border-border bg-background">privileged</Badge>}
                    </div>
                  </td>
                  <td className="px-5 py-4 tabular-nums">{r.isSystem && r.name === "super_admin" ? "all" : r.permissions.length}</td>
                  <td className="px-5 py-4 tabular-nums">{r.users}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      </Card>
    </Page>
  );
}

function PermissionMatrix({
  permissions,
  selected,
  onChange,
  disabled,
  describedBy,
}: {
  permissions: PermissionInfo[];
  selected: Set<string>;
  onChange: (next: Set<string>) => void;
  disabled?: boolean;
  describedBy?: string;
}) {
  const groups = useMemo(() => groupPermissions(permissions), [permissions]);
  return (
    <div className="grid min-w-0 grid-cols-1 items-start gap-4 md:grid-cols-2 xl:grid-cols-3">
      {groups.map(([group, list]) => (
        <fieldset key={group} className="min-w-0 rounded-lg border border-border bg-background p-3"
          aria-describedby={describedBy}>
          <legend className="max-w-full break-all px-2 text-sm font-semibold">{group}</legend>
          {list.map((p) => (
            <label key={p.name}
              className="flex min-w-0 items-start gap-3 rounded-md px-2 py-2.5 text-sm transition-colors hover:bg-muted/40 has-disabled:cursor-not-allowed has-disabled:opacity-60"
              title={p.grantable ? undefined : "You do not hold this permission"}>
              <Checkbox
                className="mt-0.5 shrink-0"
                aria-describedby={describedBy}
                checked={selected.has(p.name)}
                // Only permissions the user holds can be added or removed.
                disabled={disabled || !p.grantable}
                onChange={(e) => {
                  const next = new Set(selected);
                  if (e.target.checked) next.add(p.name);
                  else next.delete(p.name);
                  onChange(next);
                }}
              />
              <span className="min-w-0 break-all leading-5">{p.name}</span>
            </label>
          ))}
        </fieldset>
      ))}
    </div>
  );
}

export function RoleCreatePage() {
  const navigate = useNavigate();
  const mounted = useMounted();
  const permissions = usePermissions();
  const create = useCreateRole();
  const [name, setName] = useState("");
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [errors, setErrors] = useState<{ fields: Errors; form: string | null; stale?: boolean }>({
    fields: {},
    form: null
  });
  if (permissions.isLoading) return <Loading />;
  if (!permissions.data) return <ErrorState error={permissions.error} />;
  const submit = async (e: FormEvent) => {
    e.preventDefault();
    setErrors({ fields: {}, form: null, stale: false });
    try {
      const role = await create.mutateAsync({ name, permissions: [...selected].sort() });
      if (mounted.current) await navigate({ to: "/system/roles/$id", params: { id: role.id } });
    } catch (err) {
      if (mounted.current) setErrors(failure(err));
    }
  };
  return (
    <Page>
      <PageHeader title="New role" description="Name this role and choose the permissions it grants."
        eyebrow="System" />
      <form className="flex min-w-0 flex-col gap-6" onSubmit={(e) => void submit(e)} noValidate>
        {errors.form && <Alert variant="destructive">{errors.form}</Alert>}
        <Card>
          <CardHeader>
            <CardTitle>Role details</CardTitle>
            <p className="text-sm text-muted-foreground">Use a clear name to identify this access level.</p>
          </CardHeader>
          <CardContent className="max-w-lg">
            <Field id="role-name" label="Name" errors={errors.fields.name}>
              <Input id="role-name" value={name} onChange={(e) => setName(e.target.value)}
                disabled={create.isPending} aria-invalid={!!errors.fields.name}
                aria-describedby={errors.fields.name ? "role-name-error" : undefined} />
            </Field>
          </CardContent>
        </Card>
        <Card className="min-w-0">
          <CardHeader>
            <CardTitle>Permissions</CardTitle>
            <p id="permissions-help" className="text-sm text-muted-foreground">Permissions are grouped by
              resource. You can only change permissions you hold.</p>
          </CardHeader>
          <CardContent className="flex flex-col gap-5">
            {errors.fields.permissions && <Alert id="permissions-error"
              variant="destructive">{errors.fields.permissions.join(" ")}</Alert>}
            <PermissionMatrix permissions={permissions.data} selected={selected} onChange={setSelected}
              disabled={create.isPending}
              describedBy={errors.fields.permissions ? "permissions-help permissions-error" : "permissions-help"} />
          </CardContent>
        </Card>
        <div>
          <Button type="submit" disabled={create.isPending}>
            Create role
          </Button>
        </div>
      </form>
    </Page>
  );
}

export function RolePage({ id }: { id: string }) {
  const navigate = useNavigate();
  const client = useQueryClient();
  const mounted = useMounted();
  const role = useRole(id);
  const permissions = usePermissions();
  const save = useSetRolePermissions(id);
  const remove = useDeleteRole();
  const [draft, setDraft] = useState<Set<string> | null>(null);
  const [base, setBase] = useState<string[]>([]);
  const [stale, setStale] = useState(false);
  const [notice, setNotice] = useState<{ variant: "default" | "destructive"; text: string } | null>(null);
  const [confirm, setConfirm] = useState(false);
  if (role.isLoading || permissions.isLoading) return <Loading />;
  if (!role.data) return <ErrorState error={role.error} />;
  // Without the permission list (no roles.view_any), the role is shown read-only.
  const catalog: PermissionInfo[] =
    permissions.data ?? role.data.permissions.map((name) => ({ name, grantable: false }));
  const r = role.data;
  const selected = draft ?? new Set(r.permissions);
  const superAdmin = r.isSystem && r.name === "super_admin";
  const act = async (action: () => Promise<unknown>, done: string) => {
    setNotice(null);
    try {
      await action();
      if (mounted.current) setNotice({ variant: "default", text: done });
      return mounted.current;
    } catch (e) {
      if (!mounted.current) return false;
      const f = failure(e);
      setNotice({
        variant: "destructive",
        text: f.form ?? (Object.values(f.fields).flat().join(" ") || "Not saved.")
      });
      if (f.stale) {
        setStale(true);
        void client.invalidateQueries({ queryKey: ["system"] });
      }
      return false;
    }
  };
  return (
    <Page>
      <PageHeader title={r.name} description="Review and manage the permissions assigned to this role."
        eyebrow="System / Roles" actions={r.abilities.delete && (
          <Button variant="destructive" onClick={() => setConfirm(true)} disabled={remove.isPending}>
            Delete role
          </Button>
        )} />
      {r.isSystem && <div><Badge>system</Badge></div>}
      {notice && <Alert variant={notice.variant}>{notice.text}</Alert>}
      {stale && (
        <Alert variant="warning">
          Someone changed this role meanwhile; your unsaved changes are kept.{" "}
          <button
            type="button"
            className="underline"
            onClick={() => {
              setDraft(null);
              setStale(false);
              setNotice(null);
            }}
          >
            Discard my changes
          </button>
        </Alert>
      )}
      {superAdmin ? (
        <Alert>The super admin role bypasses permission checks in the admin panel; it has no permission
          list.</Alert>
      ) : (
        <Card className="min-w-0">
          <CardHeader>
            <CardTitle>Permissions</CardTitle>
            <p id="permissions-help" className="text-sm text-muted-foreground">Permissions are grouped by
              resource. You can only change permissions you hold.</p>
          </CardHeader>
          <CardContent className="flex flex-col gap-5">
            <PermissionMatrix
              permissions={catalog}
              selected={selected}
              onChange={(next) => {
                if (draft === null) setBase(r.permissions);
                setDraft(next);
              }}
              disabled={!r.abilities.update || !permissions.data || save.isPending}
              describedBy="permissions-help"
            />
            {r.abilities.update && permissions.data && (
              <div className="border-t border-border pt-5">
                <Button
                  disabled={!draft || save.isPending}
                  onClick={() => void act(() => save.mutateAsync({
                    permissions: [...selected].sort(),
                    expected: base
                  }), "Permissions saved.").then((ok) => ok && setDraft(null))}
                >
                  Save permissions
                </Button>
              </div>
            )}
          </CardContent>
        </Card>
      )}
      <Link to="/system/roles"
        className="w-fit text-sm text-muted-foreground underline-offset-4 hover:text-foreground hover:underline focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring">
        ← All roles
      </Link>
      <Dialog open={confirm} onClose={() => setConfirm(false)} title="Delete role?">
        <p className="text-sm text-muted-foreground">{r.users} users lose this role.</p>
        <div className="flex flex-wrap justify-end gap-2">
          <Button variant="outline" onClick={() => setConfirm(false)}>
            Cancel
          </Button>
          <Button
            variant="destructive"
            onClick={() => {
              setConfirm(false);
              void act(() => remove.mutateAsync(id), "Deleted.").then((ok) => {
                if (ok && mounted.current) void navigate({ to: "/system/roles" });
              });
            }}
          >
            Delete
          </Button>
        </div>
      </Dialog>
    </Page>
  );
}
