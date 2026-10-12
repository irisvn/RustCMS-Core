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
import { NotFoundPage } from "@/components/states";
import { ListPage, type ListSearch } from "@/pages/list";
import { LoginPage } from "@/pages/login";
import { FormPage, ViewPage } from "@/pages/record";
import { Dashboard, Shell } from "@/pages/shell";
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
  component: function Create() {
    const { slug } = create.useParams();
    return <FormPage key={slug} slug={slug} />;
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

const routeTree = root.addChildren([login, app.addChildren([index, list, create, view, edit])]);

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
