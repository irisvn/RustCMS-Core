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

function Field({ id, label, errors, children }: { id: string; label: string; errors?: string[]; children: React.ReactNode }) {
  return (
    <div className="flex flex-col gap-1.5">
      <Label htmlFor={id}>{label}</Label>
      {children}
      {errors && <p role="alert" className="text-xs text-destructive">{errors.join(" ")}</p>}
    </div>
  );
}

const linkButton =
  "inline-flex h-9 items-center gap-2 rounded-md bg-primary px-4 text-sm font-medium text-primary-foreground hover:bg-primary/90";

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
  if (!users.data && !(users.error instanceof ApiError && users.error.status === 422)) return <ErrorState error={users.error} />;
  const rows = users.data?.data ?? [];
  const meta = users.data?.meta ?? { page: 1, perPage: 25, total: 0, lastPage: 1 };
  return (
    <div className="flex flex-col gap-4">
      <div className="flex items-center justify-between">
        <h1 className="text-xl font-semibold">Users</h1>
        {panel.data?.system?.createUsers && (
          <Link to="/system/users/create" className={linkButton}>
            New user
          </Link>
        )}
      </div>
      <Card>
        <div className="border-b border-border p-3">
          <Input aria-label="Search users" placeholder="Search name or email…" className="max-w-xs" maxLength={100} value={search} onChange={(e) => setSearch(e.target.value)} />
          {users.error && (
            <p role="alert" className="mt-2 text-xs text-destructive">
              {users.error instanceof ApiError && users.error.status === 422
                ? (Object.values(users.error.fieldErrors)[0]?.[0] ?? users.error.message)
                : "Could not refresh the list."}
            </p>
          )}
        </div>
        <table className="w-full text-sm">
          <thead className="border-b border-border text-left text-xs text-muted-foreground">
            <tr>
              <th className="p-3 font-medium">Name</th>
              <th className="p-3 font-medium">Email</th>
              <th className="p-3 font-medium">Roles</th>
              <th className="p-3 font-medium">Status</th>
            </tr>
          </thead>
          <tbody>
            {rows.map((u) => (
              <tr key={u.id} className="border-b border-border last:border-0 hover:bg-muted/50">
                <td className="p-3">
                  <Link to="/system/users/$id" params={{ id: u.id }} className="font-medium text-primary hover:underline">
                    {u.name}
                  </Link>
                  {u.isSelf && <span className="ml-2 text-xs text-muted-foreground">(you)</span>}
                </td>
                <td className="p-3">{u.email}</td>
                <td className="p-3">
                  <div className="flex flex-wrap gap-1">
                    {u.roles.map((r) => (
                      <Badge key={r.id} color={r.name === "super_admin" ? "warning" : "primary"}>
                        {r.name}
                      </Badge>
                    ))}
                  </div>
                </td>
                <td className="p-3">{u.isActive ? <Badge color="success">active</Badge> : <Badge color="danger">disabled</Badge>}</td>
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
        <div className="flex items-center justify-between border-t border-border p-3 text-sm">
          <span className="text-muted-foreground">
            Page {meta.page} of {meta.lastPage} · {meta.total} users
          </span>
          <div className="flex gap-2">
            <Button size="sm" variant="outline" disabled={page <= 1} onClick={() => setPage(page - 1)}>
              Previous
            </Button>
            <Button size="sm" variant="outline" disabled={page >= meta.lastPage} onClick={() => setPage(page + 1)}>
              Next
            </Button>
          </div>
        </div>
      </Card>
    </div>
  );
}

function RolePicker({
  roles,
  selected,
  onChange,
  disabled,
}: {
  roles: SystemRole[];
  selected: string[];
  onChange: (ids: string[]) => void;
  disabled?: boolean;
}) {
  return (
    <fieldset className="flex flex-col gap-1.5">
      <legend className="mb-1 text-sm font-medium">Roles</legend>
      {roles.map((r) => (
        <label key={r.id} className="flex items-center gap-2 text-sm">
          <Checkbox
            checked={selected.includes(r.id)}
            disabled={disabled || !r.abilities.assign}
            onChange={(e) => onChange(e.target.checked ? [...selected, r.id] : selected.filter((id) => id !== r.id))}
          />
          {r.name}
          {r.isSystem && <ShieldCheck className="size-3 text-warning" role="img" aria-label="system role" />}
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
  const [errors, setErrors] = useState<{ fields: Errors; form: string | null; stale?: boolean }>({ fields: {}, form: null });
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
    <form className="flex max-w-xl flex-col gap-4" onSubmit={(e) => void submit(e)} noValidate>
      <h1 className="text-xl font-semibold">New user</h1>
      {created && <Alert>{created} was created.</Alert>}
      {errors.form && <Alert variant="destructive">{errors.form}</Alert>}
      <Card>
        <CardContent className="flex flex-col gap-4">
          <Field id="name" label="Name" errors={errors.fields.name}>
            <Input id="name" value={form.name} onChange={(e) => setForm({ ...form, name: e.target.value })} disabled={create.isPending} />
          </Field>
          <Field id="email" label="Email" errors={errors.fields.email}>
            <Input id="email" type="email" value={form.email} onChange={(e) => setForm({ ...form, email: e.target.value })} disabled={create.isPending} />
          </Field>
          <Field id="password" label="Password" errors={errors.fields.password}>
            <Input id="password" type="password" autoComplete="new-password" value={form.password} onChange={(e) => setForm({ ...form, password: e.target.value })} disabled={create.isPending} />
          </Field>
          {roles.data && roles.data.some((r) => r.abilities.assign) && (
            <div className="flex flex-col gap-1.5">
              <RolePicker roles={roles.data} selected={roleIds} onChange={setRoleIds} disabled={create.isPending} />
              {errors.fields.roleIds && <p role="alert" className="text-xs text-destructive">{errors.fields.roleIds.join(" ")}</p>}
            </div>
          )}
        </CardContent>
      </Card>
      <div className="flex gap-2">
        <Button type="submit" disabled={create.isPending}>
          {create.isPending ? "Creating…" : "Create user"}
        </Button>
        <Link to="/system/users" className="inline-flex h-9 items-center rounded-md border border-border px-4 text-sm">
          Cancel
        </Link>
      </div>
    </form>
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
    <div className="flex max-w-2xl flex-col gap-4">
      <div className="flex items-center justify-between">
        <h1 className="text-xl font-semibold">
          {u.name} {u.superAdmin && <Badge color="warning">super admin</Badge>}
        </h1>
        {u.abilities.delete && (
          <Button variant="destructive" onClick={() => setConfirm(true)} disabled={remove.isPending}>
            Delete user
          </Button>
        )}
      </div>
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

      <Card>
        <CardHeader>
          <CardTitle>Profile</CardTitle>
        </CardHeader>
        <CardContent>
          <form
            className="flex flex-col gap-4"
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
              <Input id="name" value={values.name} disabled={!u.abilities.update || profile.isPending} onChange={(e) => {
                  if (!draft) setProfileBase({ name: u.name, email: u.email });
                  setDraft({ ...values, name: e.target.value });
                }} />
            </Field>
            <Field id="email" label="Email" errors={errors.email}>
              <Input id="email" type="email" value={values.email} disabled={!u.abilities.changeEmail || profile.isPending} onChange={(e) => {
                  if (!draft) setProfileBase({ name: u.name, email: u.email });
                  setDraft({ ...values, email: e.target.value });
                }} />
            </Field>
            {ownEmailChange && (
              <Field id="current-password" label="Current password (to change your email)" errors={errors.current_password}>
                <Input id="current-password" type="password" autoComplete="current-password" value={currentPassword} onChange={(e) => setCurrentPassword(e.target.value)} disabled={profile.isPending} />
              </Field>
            )}
            {u.abilities.update && (
              <div>
                <Button type="submit" disabled={!draft || profile.isPending}>
                  Save profile
                </Button>
              </div>
            )}
          </form>
        </CardContent>
      </Card>

      {u.abilities.assignRoles && roles.data && (
        <Card>
          <CardHeader>
            <CardTitle>Roles</CardTitle>
          </CardHeader>
          <CardContent className="flex flex-col gap-4">
            <RolePicker
              roles={roles.data}
              selected={selectedRoles}
              onChange={(ids) => {
                if (roleDraft === null) setRoleBase(u.roles.map((r) => r.id));
                setRoleDraft(ids);
              }}
              disabled={setRoles.isPending}
            />
            {errors.roleIds && <p role="alert" className="text-xs text-destructive">{errors.roleIds.join(" ")}</p>}
            <div>
              <Button
                disabled={!roleDraft || setRoles.isPending}
                onClick={() => void run(() => setRoles.mutateAsync({ roleIds: selectedRoles, expected: roleBase }), "Roles saved.").then((ok) => ok && setRoleDraft(null))}
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
          </CardHeader>
          <CardContent className="flex flex-col gap-4">
            {u.abilities.activate && (
              <label className="flex items-center gap-3 text-sm">
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
                className="flex items-end gap-2"
                noValidate
                onSubmit={(e) => {
                  e.preventDefault();
                  void run(() => reset.mutateAsync(password), "Password set; the user was signed out everywhere.").then(
                    (ok) => ok && setPassword(""),
                  );
                }}
              >
                <Field id="new-password" label="New password" errors={errors.password}>
                  <Input id="new-password" type="password" autoComplete="new-password" value={password} onChange={(e) => setPassword(e.target.value)} disabled={reset.isPending} />
                </Field>
                <Button type="submit" variant="outline" disabled={!password || reset.isPending}>
                  Set password
                </Button>
              </form>
            )}
          </CardContent>
        </Card>
      )}

      <Link to="/system/users" className="text-sm text-primary hover:underline">
        ← All users
      </Link>
      <Dialog open={confirm} onClose={() => setConfirm(false)} title="Delete user?">
        <p className="text-sm text-muted-foreground">{u.email} will be deleted with their sessions and tokens.</p>
        <div className="flex justify-end gap-2">
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
    </div>
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
    <div className="flex flex-col gap-4">
      <div className="flex items-center justify-between">
        <h1 className="text-xl font-semibold">Roles</h1>
        {panel.data?.system?.createRoles && (
          <Link to="/system/roles/create" className={linkButton}>
            New role
          </Link>
        )}
      </div>
      <Card>
        <table className="w-full text-sm">
          <thead className="border-b border-border text-left text-xs text-muted-foreground">
            <tr>
              <th className="p-3 font-medium">Name</th>
              <th className="p-3 font-medium">Permissions</th>
              <th className="p-3 font-medium">Users</th>
            </tr>
          </thead>
          <tbody>
            {roles.data.map((r) => (
              <tr key={r.id} className="border-b border-border last:border-0 hover:bg-muted/50">
                <td className="p-3">
                  <Link to="/system/roles/$id" params={{ id: r.id }} className="font-medium text-primary hover:underline">
                    {r.name}
                  </Link>{" "}
                  {r.isSystem && <Badge color="warning">system</Badge>} {r.isPrivileged && <Badge color="danger">privileged</Badge>}
                </td>
                <td className="p-3">{r.isSystem && r.name === "super_admin" ? "all" : r.permissions.length}</td>
                <td className="p-3">{r.users}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </Card>
    </div>
  );
}

function PermissionMatrix({
  permissions,
  selected,
  onChange,
  disabled,
}: {
  permissions: PermissionInfo[];
  selected: Set<string>;
  onChange: (next: Set<string>) => void;
  disabled?: boolean;
}) {
  const groups = useMemo(() => groupPermissions(permissions), [permissions]);
  return (
    <div className="grid grid-cols-1 gap-4 md:grid-cols-2">
      {groups.map(([group, list]) => (
        <fieldset key={group} className="rounded-md border border-border p-3">
          <legend className="px-1 text-sm font-semibold">{group}</legend>
          {list.map((p) => (
            <label key={p.name} className="flex items-center gap-2 text-sm" title={p.grantable ? undefined : "You do not hold this permission"}>
              <Checkbox
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
              {p.name}
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
  const [errors, setErrors] = useState<{ fields: Errors; form: string | null; stale?: boolean }>({ fields: {}, form: null });
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
    <form className="flex flex-col gap-4" onSubmit={(e) => void submit(e)} noValidate>
      <h1 className="text-xl font-semibold">New role</h1>
      {errors.form && <Alert variant="destructive">{errors.form}</Alert>}
      <Card>
        <CardContent className="flex flex-col gap-4">
          <Field id="role-name" label="Name" errors={errors.fields.name}>
            <Input id="role-name" className="max-w-sm" value={name} onChange={(e) => setName(e.target.value)} disabled={create.isPending} />
          </Field>
          {errors.fields.permissions && <Alert variant="destructive">{errors.fields.permissions.join(" ")}</Alert>}
          <PermissionMatrix permissions={permissions.data} selected={selected} onChange={setSelected} disabled={create.isPending} />
        </CardContent>
      </Card>
      <div>
        <Button type="submit" disabled={create.isPending}>
          Create role
        </Button>
      </div>
    </form>
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
      setNotice({ variant: "destructive", text: f.form ?? (Object.values(f.fields).flat().join(" ") || "Not saved.") });
      if (f.stale) {
        setStale(true);
        void client.invalidateQueries({ queryKey: ["system"] });
      }
      return false;
    }
  };
  return (
    <div className="flex flex-col gap-4">
      <div className="flex items-center justify-between">
        <h1 className="text-xl font-semibold">
          {r.name} {r.isSystem && <Badge color="warning">system</Badge>}
        </h1>
        {r.abilities.delete && (
          <Button variant="destructive" onClick={() => setConfirm(true)} disabled={remove.isPending}>
            Delete role
          </Button>
        )}
      </div>
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
        <Alert>The super admin role bypasses permission checks in the admin panel; it has no permission list.</Alert>
      ) : (
        <Card>
          <CardContent className="flex flex-col gap-4">
            <PermissionMatrix
              permissions={catalog}
              selected={selected}
              onChange={(next) => {
                if (draft === null) setBase(r.permissions);
                setDraft(next);
              }}
              disabled={!r.abilities.update || !permissions.data || save.isPending}
            />
            {r.abilities.update && permissions.data && (
              <div>
                <Button
                  disabled={!draft || save.isPending}
                  onClick={() => void act(() => save.mutateAsync({ permissions: [...selected].sort(), expected: base }), "Permissions saved.").then((ok) => ok && setDraft(null))}
                >
                  Save permissions
                </Button>
              </div>
            )}
          </CardContent>
        </Card>
      )}
      <Link to="/system/roles" className="text-sm text-primary hover:underline">
        ← All roles
      </Link>
      <Dialog open={confirm} onClose={() => setConfirm(false)} title="Delete role?">
        <p className="text-sm text-muted-foreground">{r.users} users lose this role.</p>
        <div className="flex justify-end gap-2">
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
    </div>
  );
}
