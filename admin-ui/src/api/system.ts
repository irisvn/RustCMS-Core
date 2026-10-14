// System pages: users, roles and permissions (`/api/system/…`) and the account page.
import { keepPreviousData, useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useLocale } from "@/i18n";
import { useApi, useSession } from "./context";
import { guardedWrite } from "./queries";
import type { PermissionInfo, SystemRole, SystemUser, UserPage } from "./types";

/** Write scopes (see `guardedWrite`): a write with an unknown outcome pauses its scope. */
export const USERS = "system:users";
export const ROLES = "system:roles";
export const ACCOUNT = "account";
const enc = encodeURIComponent;

export function useUsers(search: string, page: number) {
  const api = useApi();
  return useQuery({
    queryKey: ["system", "users", search, page],
    queryFn: () => api.get<UserPage>("/system/users", { search: search || undefined, page: String(page) }),
    placeholderData: keepPreviousData,
  });
}

export function useUser(id: string) {
  const api = useApi();
  return useQuery({ queryKey: ["system", "user", id], queryFn: () => api.get<SystemUser>(`/system/users/${enc(id)}`) });
}

export function useRoles(enabled = true) {
  const api = useApi();
  return useQuery({ queryKey: ["system", "roles"], queryFn: () => api.get<SystemRole[]>("/system/roles"), enabled });
}

export function useRole(id: string) {
  const api = useApi();
  return useQuery({ queryKey: ["system", "role", id], queryFn: () => api.get<SystemRole>(`/system/roles/${enc(id)}`) });
}

export function usePermissions() {
  const api = useApi();
  // Group labels come translated.
  const locale = useLocale();
  return useQuery({
    queryKey: ["system", "permissions", locale],
    queryFn: () => api.get<PermissionInfo[]>("/system/permissions"),
    placeholderData: keepPreviousData,
  });
}

/** A system write: serialized and paused like resource writes; refreshes system data and the panel. */
function useSystemWrite<I, O>(scope: string, write: (input: I) => Promise<O>) {
  const session = useSession();
  const client = useQueryClient();
  return useMutation({
    mutationFn: (input: I) => guardedWrite(session, scope, () => write(input)),
    // Users and roles show up elsewhere too (labels, options, abilities, the signed-in user).
    onSuccess: () => client.invalidateQueries(),
  });
}

export function useCreateUser() {
  const api = useApi();
  return useSystemWrite(USERS, (input: { name: string; email: string; password: string; roleIds: string[] }) =>
    // No body (204) when the creator may not view users.
    api.post<SystemUser | undefined>("/system/users", input),
  );
}

export function useUpdateProfile(id: string) {
  const api = useApi();
  const session = useSession();
  // `expected`: the profile the editor started from; refused if it changed meanwhile.
  return useSystemWrite(USERS, (input: { name: string; email: string; expected: { name: string; email: string }; currentPassword?: string }) =>
    // Changing one's own email re-issues this session (new CSRF token, other sessions end).
    input.currentPassword !== undefined
      ? session.rotating(() => api.patch<SystemUser | undefined>(`/system/users/${enc(id)}`, input))
      : api.patch<SystemUser | undefined>(`/system/users/${enc(id)}`, input),
  );
}

export function useSetUserRoles(id: string) {
  const api = useApi();
  // `expected`: the roles the editor started from; the server refuses if they changed meanwhile.
  return useSystemWrite(USERS, ({ roleIds, expected }: { roleIds: string[]; expected: string[] }) =>
    api.request<SystemUser>("PUT", `/system/users/${enc(id)}/roles`, { roleIds, expectedRoleIds: expected }),
  );
}

export function useSetActive(id: string) {
  const api = useApi();
  // `expected`: the status shown; refused if it changed meanwhile.
  return useSystemWrite(USERS, ({ active, expected }: { active: boolean; expected: boolean }) =>
    api.request<SystemUser | undefined>("PUT", `/system/users/${enc(id)}/active`, { active, expected }),
  );
}

export function useResetPassword(id: string) {
  const api = useApi();
  return useSystemWrite(USERS, (password: string) =>
    api.request<void>("PUT", `/system/users/${enc(id)}/password`, { password }),
  );
}

export function useDeleteUser() {
  const api = useApi();
  return useSystemWrite(USERS, (id: string) => api.delete<void>(`/system/users/${enc(id)}`));
}

export function useCreateRole() {
  const api = useApi();
  return useSystemWrite(ROLES, (input: { name: string; permissions: string[] }) => api.post<SystemRole>("/system/roles", input));
}

export function useSetRolePermissions(id: string) {
  const api = useApi();
  return useSystemWrite(ROLES, ({ permissions, expected }: { permissions: string[]; expected: string[] }) =>
    api.request<SystemRole>("PUT", `/system/roles/${enc(id)}/permissions`, { permissions, expectedPermissions: expected }),
  );
}

export function useDeleteRole() {
  const api = useApi();
  return useSystemWrite(ROLES, (id: string) => api.delete<void>(`/system/roles/${enc(id)}`));
}

// Both rotate the session on the server. A new epoch starts *before* sending
// (requests already in flight with the old cookie become obsolete: their 401s
// never sign the tab out) and again once the answer is in, whatever it is.
function useRotation<I>(path: string, body: (input: I) => unknown) {
  const api = useApi();
  const session = useSession();
  return useSystemWrite(ACCOUNT, (input: I) => session.rotating(() => api.post<void>(path, body(input))));
}

export function useChangePassword() {
  return useRotation("/auth/password", (input: { current_password: string; password: string; password_confirmation: string }) => input);
}

export function useLogoutOthers() {
  return useRotation("/auth/logout-others", (password: string) => ({ password }));
}

/** Permissions by group, groups and names sorted. */
export function groupPermissions(permissions: PermissionInfo[]): [string, PermissionInfo[]][] {
  const groups = new Map<string, PermissionInfo[]>();
  for (const p of permissions) {
    const group = p.group ?? p.name.split(".")[0] ?? "other";
    groups.set(group, [...(groups.get(group) ?? []), p]);
  }
  return [...groups.entries()]
    .sort(([a], [b]) => a.localeCompare(b))
    .map(([g, list]) => [g, [...list].sort((a, b) => a.name.localeCompare(b.name))]);
}
