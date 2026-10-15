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
import { later, show, type Text, useT } from "@/i18n";

export function RolesPage() {
  const t = useT();
  const panel = usePanel();
  const roles = useRoles();
  if (roles.isLoading) return <Loading />;
  if (!roles.data) return <ErrorState error={roles.error} />;
  return (
    <Page>
      <PageHeader title={t("nav.roles")} description={t("roles.description")}
        eyebrow={t("nav.system")} actions={panel.data?.system?.createRoles && (
          <Link to="/system/roles/create" className={linkButton}>
            {t("roles.new")}
          </Link>
        )} />
      <PausedBanner scope={ROLES} />
      <Card className="min-w-0 overflow-hidden">
        <div className="overflow-x-auto">
          <table aria-label={t("nav.roles")} className="w-full min-w-[480px] text-sm">
            <thead className="border-b border-border bg-muted/40 text-left text-xs text-muted-foreground">
              <tr>
                <th scope="col" className="px-5 py-3 font-medium">{t("field.name")}</th>
                <th scope="col" className="px-5 py-3 font-medium">{t("roles.permissions")}</th>
                <th scope="col" className="px-5 py-3 font-medium">{t("nav.users")}</th>
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
                      {r.isSystem && <Badge>{t("roles.system")}</Badge>} {r.isPrivileged &&
                        <Badge className="border border-border bg-background">{t("roles.privileged")}</Badge>}
                    </div>
                  </td>
                  <td className="px-5 py-4 tabular-nums">{r.isSystem && r.name === "super_admin" ? t("roles.all") : r.permissions.length}</td>
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

/** Columns of the matrix, in this order (only those some resource uses). */
const ABILITIES = ["view_any", "view", "create", "update", "delete", "restore", "force_delete"] as const;

function abilityLabel(t: ReturnType<typeof useT>, ability: string): string {
  switch (ability) {
    case "view_any": return t("ability.view_any");
    case "view": return t("ability.view");
    case "create": return t("ability.create");
    case "update": return t("ability.update");
    case "delete": return t("ability.delete");
    case "restore": return t("ability.restore");
    case "force_delete": return t("ability.force_delete");
    default: return ability;
  }
}

/**
 * Permissions as a resource × ability matrix (the server names each cell),
 * other abilities of a resource in its last column, and everything else
 * (system, settings, audit…) by group. Permissions the user does not hold
 * stay as they are.
 */
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
  const t = useT();
  const { rows, columns, groups } = useMemo(() => {
    type Row = { label: string; cells: Map<string, PermissionInfo>; other: PermissionInfo[] };
    const byResource = new Map<string, Row>();
    const rest: PermissionInfo[] = [];
    for (const p of permissions) {
      if (!p.resource || !p.ability) {
        rest.push(p);
        continue;
      }
      const row: Row = byResource.get(p.resource) ?? { label: p.resource, cells: new Map(), other: [] };
      if (p.group === p.resource && p.groupLabel) row.label = p.groupLabel;
      if ((ABILITIES as readonly string[]).includes(p.ability)) row.cells.set(p.ability, p);
      else row.other.push(p);
      byResource.set(p.resource, row);
    }
    const rows = [...byResource.entries()].sort(([, a], [, b]) => a.label.localeCompare(b.label));
    const columns = ABILITIES.filter((a) => rows.some(([, r]) => r.cells.has(a)));
    return { rows, columns, groups: groupPermissions(rest) };
  }, [permissions]);
  const set = (list: PermissionInfo[], on: boolean) => {
    const next = new Set(selected);
    for (const p of list) {
      // Only permissions the user holds can be added or removed.
      if (!p.grantable) continue;
      if (on) next.add(p.name);
      else next.delete(p.name);
    }
    onChange(next);
  };
  const box = (p: PermissionInfo, label: string) => (
    <Checkbox
      aria-label={label}
      aria-describedby={describedBy}
      title={p.grantable ? undefined : t("roles.notHeld")}
      checked={selected.has(p.name)}
      disabled={disabled || !p.grantable}
      onChange={(e) => set([p], e.target.checked)}
    />
  );
  // A whole row or column reads (and switches) what the user may grant; the
  // others keep their own state.
  const all = (list: PermissionInfo[]) => {
    const own = list.filter((p) => p.grantable);
    return own.length > 0 && own.every((p) => selected.has(p.name));
  };
  const grantable = (list: PermissionInfo[]) => list.some((p) => p.grantable);
  return (
    <div className="flex min-w-0 flex-col gap-6">
      {rows.length > 0 && (
        <div className="max-w-full overflow-x-auto rounded-lg border border-border">
          <table className="w-full text-sm" aria-describedby={describedBy}>
            <caption className="sr-only">{t("roles.matrix")}</caption>
            <thead className="border-b border-border bg-muted/40 text-left text-xs text-muted-foreground">
              <tr>
                <th scope="col" className="sticky left-0 bg-muted p-3 font-medium">{t("roles.matrixResource")}</th>
                {columns.map((ability) => {
                  const list = rows.flatMap(([, r]) => (r.cells.get(ability) ? [r.cells.get(ability)!] : []));
                  return (
                    <th key={ability} scope="col" className="p-3 text-center font-medium">
                      <label className="flex flex-col items-center gap-1">
                        <span>{abilityLabel(t, ability)}</span>
                        <Checkbox
                          aria-label={t("roles.matrixColumn", { ability: abilityLabel(t, ability) })}
                          checked={all(list)}
                          disabled={disabled || !grantable(list)}
                          onChange={(e) => set(list, e.target.checked)}
                        />
                      </label>
                    </th>
                  );
                })}
                <th scope="col" className="p-3 font-medium">{t("roles.matrixOther")}</th>
              </tr>
            </thead>
            <tbody>
              {rows.map(([resource, row]) => {
                const list = [...row.cells.values(), ...row.other];
                return (
                  <tr key={resource} className="border-b border-border last:border-0">
                    <th scope="row" className="sticky left-0 bg-background p-3 text-left font-medium">
                      <label className="flex items-center gap-2">
                        <Checkbox
                          aria-label={t("roles.matrixRow", { resource: row.label })}
                          checked={all(list)}
                          disabled={disabled || !grantable(list)}
                          onChange={(e) => set(list, e.target.checked)}
                        />
                        <span className="break-all">{row.label}</span>
                      </label>
                    </th>
                    {columns.map((ability) => {
                      const p = row.cells.get(ability);
                      return (
                        <td key={ability} className="p-3 text-center">
                          {p && box(p, t("roles.matrixCell", { resource: row.label, ability: abilityLabel(t, ability) }))}
                        </td>
                      );
                    })}
                    <td className="p-3">
                      <div className="flex flex-wrap gap-x-4 gap-y-2">
                        {row.other.map((p) => (
                          <label key={p.name} className="flex items-center gap-2 has-disabled:opacity-60">
                            {box(p, t("roles.matrixCell", { resource: row.label, ability: p.ability ?? p.name }))}
                            <span className="break-all text-xs">{p.ability}</span>
                          </label>
                        ))}
                      </div>
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        </div>
      )}
      {groups.length > 0 && (
        <div className="grid min-w-0 grid-cols-1 items-start gap-4 md:grid-cols-2 xl:grid-cols-3">
          {groups.map(([group, list]) => (
            <fieldset key={group} className="min-w-0 rounded-lg border border-border bg-background p-3"
              aria-describedby={describedBy}>
              <legend className="max-w-full break-all px-2 text-sm font-semibold">{list[0]?.groupLabel ?? group}</legend>
              {list.map((p) => (
                <label key={p.name}
                  className="flex min-w-0 items-start gap-3 rounded-md px-2 py-2.5 text-sm transition-colors hover:bg-muted/40 has-disabled:cursor-not-allowed has-disabled:opacity-60"
                  title={p.grantable ? undefined : t("roles.notHeld")}>
                  <Checkbox
                    className="mt-0.5 shrink-0"
                    aria-describedby={describedBy}
                    checked={selected.has(p.name)}
                    disabled={disabled || !p.grantable}
                    onChange={(e) => set([p], e.target.checked)}
                  />
                  <span className="min-w-0 break-all leading-5">{p.name}</span>
                </label>
              ))}
            </fieldset>
          ))}
        </div>
      )}
    </div>
  );
}

export function RoleCreatePage() {
  const t = useT();
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
      <PageHeader title={t("roles.new")} description={t("roles.newDescription")}
        eyebrow={t("nav.system")} />
      <PausedBanner scope={ROLES} />
      <form className="flex min-w-0 flex-col gap-6" onSubmit={(e) => void submit(e)} noValidate>
        {errors.form && <Alert variant="destructive">{errors.form}</Alert>}
        <Card>
          <CardHeader>
            <CardTitle>{t("roles.details")}</CardTitle>
            <p className="text-sm text-muted-foreground">{t("roles.detailsText")}</p>
          </CardHeader>
          <CardContent className="max-w-lg">
            <Field id="role-name" label={t("field.name")} errors={errors.fields.name}>
              <Input id="role-name" value={name} onChange={(e) => setName(e.target.value)}
                disabled={create.isPending} aria-invalid={!!errors.fields.name}
                aria-describedby={errors.fields.name ? "role-name-error" : undefined} />
            </Field>
          </CardContent>
        </Card>
        <Card className="min-w-0">
          <CardHeader>
            <CardTitle>{t("roles.permissions")}</CardTitle>
            <p id="permissions-help" className="text-sm text-muted-foreground">{t("roles.permissionsHelp")}</p>
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
            {t("roles.create")}
          </Button>
        </div>
      </form>
    </Page>
  );
}

export function RolePage({ id }: { id: string }) {
  const t = useT();
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
  const [notice, setNotice] = useState<{ variant: "default" | "destructive"; text: Text } | null>(null);
  const [confirm, setConfirm] = useState(false);
  if (role.isLoading || permissions.isLoading) return <Loading />;
  if (!role.data) return <ErrorState error={role.error} />;
  // Without the permission list (no roles.view_any), the role is shown read-only.
  const catalog: PermissionInfo[] =
    permissions.data ?? role.data.permissions.map((name) => ({ name, grantable: false }));
  const r = role.data;
  const selected = draft ?? new Set(r.permissions);
  const superAdmin = r.isSystem && r.name === "super_admin";
  const act = async (action: () => Promise<unknown>, done: Text) => {
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
        text: () => f.form ?? (Object.values(f.fields).flat().join(" ") || t("system.notSaved")),
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
      <PageHeader title={r.name} description={t("roles.roleDescription")}
        eyebrow={t("roles.eyebrow")} actions={r.abilities.delete && (
          <Button variant="destructive" onClick={() => setConfirm(true)} disabled={remove.isPending}>
            {t("roles.delete")}
          </Button>
        )} />
      <PausedBanner scope={ROLES} />
      {r.isSystem && <div><Badge>{t("roles.system")}</Badge></div>}
      {notice && <Alert variant={notice.variant}>{show(notice.text)}</Alert>}
      {stale && (
        <Alert variant="warning">
          {t("roles.stale")}{" "}
          <button
            type="button"
            className="underline"
            onClick={() => {
              setDraft(null);
              setStale(false);
              setNotice(null);
            }}
          >
            {t("system.discard")}
          </button>
        </Alert>
      )}
      {superAdmin ? (
        <Alert>{t("roles.superAdmin")}</Alert>
      ) : (
        <Card className="min-w-0">
          <CardHeader>
            <CardTitle>{t("roles.permissions")}</CardTitle>
            <p id="permissions-help" className="text-sm text-muted-foreground">{t("roles.permissionsHelp")}</p>
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
                  }), later("roles.saved")).then((ok) => ok && setDraft(null))}
                >
                  {t("roles.save")}
                </Button>
              </div>
            )}
          </CardContent>
        </Card>
      )}
      <Link to="/system/roles"
        className="w-fit text-sm text-muted-foreground underline-offset-4 hover:text-foreground hover:underline focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring">
        {t("roles.back")}
      </Link>
      <Dialog open={confirm} onClose={() => setConfirm(false)} title={t("roles.confirmDelete")}>
        <p className="text-sm text-muted-foreground">{t("roles.usersLose", { count: r.users })}</p>
        <div className="flex flex-wrap justify-end gap-2">
          <Button variant="outline" onClick={() => setConfirm(false)}>
            {t("action.cancel")}
          </Button>
          <Button
            variant="destructive"
            onClick={() => {
              setConfirm(false);
              void act(() => remove.mutateAsync(id), later("list.deleted")).then((ok) => {
                if (ok && mounted.current) void navigate({ to: "/system/roles" });
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
