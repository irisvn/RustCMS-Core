// System roles: list, create, edit permissions.
import { Link, useNavigate } from "@tanstack/react-router";
import { useQueryClient } from "@tanstack/react-query";
import { type FormEvent, useMemo, useState } from "react";
import { usePanel } from "@/api/queries";
import { ROLES, groupPermissions, useCreateRole, useDeleteRole, usePermissions, useRole, useRoles, useSetRolePermissions } from "@/api/system";
import type { PermissionInfo } from "@/api/types";
import { Page, PageHeader } from "@/components/layout";
import { ErrorState, Loading, PausedBanner } from "@/components/states";
import { Alert } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Checkbox } from "@/components/ui/checkbox";
import { Dialog } from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";

import { Field, type Errors, failure, linkButton, useMounted } from "./shared";

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
      <PausedBanner scope={ROLES} />
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
      <PausedBanner scope={ROLES} />
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
      <PausedBanner scope={ROLES} />
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
