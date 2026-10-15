import type { QueryClient } from "@tanstack/react-query";
import {
  Outlet,
  createRootRouteWithContext,
  createRoute,
  createRouter,
  redirect,
  type RouterHistory,
} from "@tanstack/react-router";
import { ApiError } from "@/api/client";
import { meQuery } from "@/api/queries";
import { serverLocale } from "@/i18n/preference";
import { NotFoundPage } from "@/components/states";
import { ListPage, type ListSearch } from "@/pages/list";
import { LoginPage } from "@/pages/login";
import { FormPage, ViewPage } from "@/pages/record";
import { Dashboard, Shell } from "@/pages/shell";
import { AccountPage } from "@/pages/account";
import { SettingsPage } from "@/pages/settings";
import { AuditLogPage } from "@/pages/audit";
import { QueuePage } from "@/pages/queue";
import { CustomPage } from "@/pages/custom";
import { ImportPage } from "@/pages/import";
import { RoleCreatePage, RolePage, RolesPage, UserCreatePage, UserPage, UsersPage } from "@/pages/system";
import type { Session } from "./session";

export interface RouterContext {
  queryClient: QueryClient;
  session: Session;
}

const root = createRootRouteWithContext<RouterContext>()({
  component: Outlet,
  notFoundComponent: NotFoundPage,
});

const login = createRoute({
  getParentRoute: () => root,
  path: "/login",
  validateSearch: (search: Record<string, unknown>): { redirect?: string } =>
    typeof search.redirect === "string" ? { redirect: search.redirect } : {},
  component: function Login() {
    const { redirect: target } = login.useSearch();
    return <LoginPage redirect={target} />;
  },
});

/** Everything behind sign-in. A 401 from `/auth/me` sends to the login page. */
const app = createRoute({
  getParentRoute: () => root,
  id: "app",
  beforeLoad: async ({ context, location }) => {
    try {
      const me = await context.queryClient.ensureQueryData(meQuery(context.session.api));
      if (typeof me.user?.id === "string") context.session.identify(me.user.id);
      // The user's language before anything is fetched in another one.
      serverLocale(me.locale, context.queryClient.getQueryState(meQuery(context.session.api).queryKey)?.dataUpdatedAt ?? 0);
    } catch (e) {
      if (e instanceof ApiError && e.status === 401) {
        throw redirect({ to: "/login", search: { redirect: context.session.config.basePath + location.href }, replace: true });
      }
      throw e;
    }
  },
  component: Shell,
});

const index = createRoute({ getParentRoute: () => app, path: "/", component: Dashboard });

const list = createRoute({
  getParentRoute: () => app,
  path: "/r/$slug",
  validateSearch: (search: Record<string, unknown>): ListSearch =>
    Object.fromEntries(Object.entries(search).filter((e): e is [string, string] => typeof e[1] === "string")),
  component: function List() {
    const { slug } = list.useParams();
    const search = list.useSearch();
    return <ListPage key={slug} slug={slug} search={search} />;
  },
});

const create = createRoute({
  getParentRoute: () => app,
  path: "/r/$slug/create",
  // Creating through a relation: `?via=<parent>&record=<key>&relation=<name>` (checked against the parent's schema).
  validateSearch: (search: Record<string, unknown>): { via?: string; record?: string; relation?: string } => {
    const text = (v: unknown) => (typeof v === "string" && v ? v : undefined);
    const [via, record, relation] = [text(search.via), text(search.record), text(search.relation)];
    return via && record && relation ? { via, record, relation } : {};
  },
  component: function Create() {
    const { slug } = create.useParams();
    const search = create.useSearch();
    const via = search.via && search.record && search.relation
      ? { parent: search.via, record: search.record, relation: search.relation }
      : undefined;
    return <FormPage key={`${slug}:${via ? `${via.parent}:${via.record}:${via.relation}` : ""}`} slug={slug} via={via} />;
  },
});

const importPage = createRoute({
  getParentRoute: () => app,
  path: "/r/$slug/import",
  component: function Import() {
    const { slug } = importPage.useParams();
    return <ImportPage key={slug} slug={slug} />;
  },
});

const view = createRoute({
  getParentRoute: () => app,
  path: "/r/$slug/records/$key",
  component: function View() {
    const { slug, key } = view.useParams();
    return <ViewPage key={`${slug}:${key}`} slug={slug} recordKey={key} />;
  },
});

const edit = createRoute({
  getParentRoute: () => app,
  path: "/r/$slug/records/$key/edit",
  component: function Edit() {
    const { slug, key } = edit.useParams();
    return <FormPage key={`${slug}:${key}`} slug={slug} recordKey={key} />;
  },
});

const systemUsers = createRoute({ getParentRoute: () => app, path: "/system/users", component: UsersPage });
const systemUserCreate = createRoute({ getParentRoute: () => app, path: "/system/users/create", component: UserCreatePage });
const systemUser = createRoute({
  getParentRoute: () => app,
  path: "/system/users/$id",
  component: function SystemUser() {
    const { id } = systemUser.useParams();
    return <UserPage key={id} id={id} />;
  },
});
const systemRoles = createRoute({ getParentRoute: () => app, path: "/system/roles", component: RolesPage });
const systemRoleCreate = createRoute({ getParentRoute: () => app, path: "/system/roles/create", component: RoleCreatePage });
const systemRole = createRoute({
  getParentRoute: () => app,
  path: "/system/roles/$id",
  component: function SystemRole() {
    const { id } = systemRole.useParams();
    return <RolePage key={id} id={id} />;
  },
});
const systemAudit = createRoute({ getParentRoute: () => app, path: "/system/audit", component: AuditLogPage });
const systemQueue = createRoute({ getParentRoute: () => app, path: "/system/queue", component: QueuePage });
const customPage = createRoute({
  getParentRoute: () => app,
  path: "/p/$slug",
  component: function Custom() {
    const { slug } = customPage.useParams();
    return <CustomPage key={slug} />;
  },
});
const settings = createRoute({
  getParentRoute: () => app,
  path: "/settings/$group",
  component: function Settings() {
    const { group } = settings.useParams();
    return <SettingsPage key={group} group={group} />;
  },
});
const account = createRoute({ getParentRoute: () => app, path: "/account", component: AccountPage });

const routeTree = root.addChildren([
  login,
  app.addChildren([
    index,
    list,
    create,
    importPage,
    view,
    edit,
    systemUsers,
    systemUserCreate,
    systemUser,
    systemRoles,
    systemRoleCreate,
    systemRole,
    systemAudit,
    systemQueue,
    customPage,
    settings,
    account,
  ]),
]);

/** Plain `a=b&c=d` search strings (the list query string is sent to the API as is). */
function parseSearch(search: string): Record<string, unknown> {
  return Object.fromEntries(new URLSearchParams(search));
}

function stringifySearch(search: Record<string, unknown>): string {
  const params = new URLSearchParams();
  for (const [k, v] of Object.entries(search)) {
    if (v !== undefined && v !== null && v !== "") params.set(k, String(v));
  }
  const s = params.toString();
  return s ? `?${s}` : "";
}

export function createAppRouter(context: RouterContext, history?: RouterHistory) {
  return createRouter({
    routeTree,
    context,
    history,
    basepath: context.session.config.basePath || "/",
    parseSearch,
    stringifySearch,
    defaultPreload: false,
    scrollRestoration: true,
  });
}

declare module "@tanstack/react-router" {
  interface Register {
    router: ReturnType<typeof createAppRouter>;
  }
}
