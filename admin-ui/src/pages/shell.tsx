import { Link, Outlet, useRouterState } from "@tanstack/react-router";
import { ArrowUpRight, ChevronRight, Home, LogOut, Menu, PanelLeftClose, PanelLeftOpen, PanelsTopLeft, Settings2, X } from "lucide-react";
import { useEffect, useState } from "react";
import { ApiError } from "@/api/client";
import { useSession } from "@/api/context";
import { meQuery, usePanel } from "@/api/queries";
import { useQuery } from "@tanstack/react-query";
import { errorText, show, type Text, useLocale, useT } from "@/i18n";
import { serverLocale } from "@/i18n/preference";
import type { PanelResponse, ResourceInfo } from "@/api/types";
import { ResourceIcon } from "@/components/icons";
import { Page, PageHeader } from "@/components/layout";
import { ErrorState, Loading } from "@/components/states";
import { Alert } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import { Dialog } from "@/components/ui/dialog";
import { cn } from "@/lib/utils";

const navLink = "flex min-h-10 items-center gap-3 rounded-md px-3 py-2 text-sm text-muted-foreground transition-colors hover:bg-muted hover:text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring [&.active]:bg-muted [&.active]:font-medium [&.active]:text-foreground data-[status=active]:bg-muted data-[status=active]:font-medium data-[status=active]:text-foreground";
const activeProps = { "aria-current": "page" as const };

function groups(navigation: ResourceInfo[]): [string, ResourceInfo[]][] {
  const out = new Map<string, ResourceInfo[]>();
  for (const r of navigation) {
    const g = r.group ?? "";
    out.set(g, [...(out.get(g) ?? []), r]);
  }
  return [...out.entries()];
}

function Navigation({ panel, collapsed = false, onNavigate }: { panel: PanelResponse; collapsed?: boolean; onNavigate?: () => void }) {
  const t = useT();
  const groupLabel = (label: string) => <p className={cn("px-3 pb-1 pt-3 text-xs font-medium text-muted-foreground", collapsed && "sr-only")}>{label}</p>;
  const linkStyle = cn(navLink, collapsed && "justify-center px-2");
  return (
    <nav aria-label={t("nav.resources")} onClick={(e) => { if ((e.target as Element).closest("a")) onNavigate?.(); }} className="flex min-h-0 flex-1 flex-col gap-4 overflow-y-auto">
      <div className="space-y-1">
        {!collapsed && groupLabel(t("nav.workspace"))}
        <Link to="/" className={linkStyle} activeProps={activeProps} activeOptions={{ exact: true }} title={collapsed ? t("nav.overview") : undefined}>
          <Home className="size-4 shrink-0" /><span className={cn("truncate", collapsed && "sr-only")}>{t("nav.overview")}</span>
        </Link>
      </div>
      {groups(panel.navigation).map(([group, items]) => (
        <div key={group} className="space-y-1">
          {group && groupLabel(group)}
          {items.map((r) => (
            <Link key={r.slug} to="/r/$slug" params={{ slug: r.slug }} className={linkStyle} activeProps={activeProps} title={collapsed ? r.pluralLabel : undefined}>
              <ResourceIcon name={r.icon} /><span className={cn("truncate", collapsed && "sr-only")}>{r.pluralLabel}</span>
            </Link>
          ))}
        </div>
      ))}
      {(panel.system?.users || panel.system?.roles || panel.system?.audit || panel.system?.queue) && (
        <div className="space-y-1">
          {groupLabel(t("nav.system"))}
          {panel.system?.users && <Link to="/system/users" className={linkStyle} activeProps={activeProps} title={collapsed ? t("nav.users") : undefined}><ResourceIcon name="users" /><span className={cn(collapsed && "sr-only")}>{t("nav.users")}</span></Link>}
          {panel.system?.roles && <Link to="/system/roles" className={linkStyle} activeProps={activeProps} title={collapsed ? t("nav.roles") : undefined}><ResourceIcon name="shield" /><span className={cn(collapsed && "sr-only")}>{t("nav.roles")}</span></Link>}
          {panel.system?.audit && <Link to="/system/audit" className={linkStyle} activeProps={activeProps} title={collapsed ? t("nav.audit") : undefined}><ResourceIcon name="history" /><span className={cn(collapsed && "sr-only")}>{t("nav.audit")}</span></Link>}
          {panel.system?.queue && <Link to="/system/queue" className={linkStyle} activeProps={activeProps} title={collapsed ? t("nav.queue") : undefined}><ResourceIcon name="layers" /><span className={cn(collapsed && "sr-only")}>{t("nav.queue")}</span></Link>}
        </div>
      )}
      {(panel.settings ?? []).length > 0 && (
        <div className="space-y-1">
          {groupLabel(t("nav.settings"))}
          {(panel.settings ?? []).map((s) => (
            <Link key={s.group} to="/settings/$group" params={{ group: s.group }} className={linkStyle} activeProps={activeProps} title={collapsed ? s.label : undefined}>
              <ResourceIcon name={s.icon ?? "settings"} /><span className={cn("truncate", collapsed && "sr-only")}>{s.label}</span>
            </Link>
          ))}
        </div>
      )}
    </nav>
  );
}

function UserFooter({ panel, collapsed, leaving, logout, error, onNavigate }: {
  panel: PanelResponse; collapsed?: boolean; leaving: boolean; logout: () => void; error: string | null; onNavigate?: () => void;
}) {
  const t = useT();
  return (
    <div className="mt-auto shrink-0 space-y-2 border-t border-border pt-3">
      <Link to="/account" onClick={onNavigate} activeProps={activeProps} title={panel.user.email} className={cn(navLink, collapsed && "justify-center px-2")}>
        <span className="flex size-8 shrink-0 items-center justify-center rounded-lg border border-border bg-card text-xs font-semibold text-foreground" aria-hidden="true">{panel.user.name.slice(0, 2).toUpperCase()}</span>
        <span className={cn("min-w-0 flex-1", collapsed && "sr-only")}><span className="block truncate font-medium text-foreground">{panel.user.name}</span><span className="block truncate text-xs">{panel.user.email}</span></span>
        <Settings2 className={cn("size-4 shrink-0", collapsed && "hidden")} />
      </Link>
      <Button variant="ghost" size={collapsed ? "icon" : "default"} className={cn("w-full text-muted-foreground", !collapsed && "justify-start px-3")} onClick={logout} disabled={leaving} title={collapsed ? t("nav.signOut") : undefined}>
        <LogOut /><span className={cn(collapsed && "sr-only")}>{t("nav.signOut")}</span>
      </Button>
      {error && <Alert variant="destructive" className="break-words">{error}</Alert>}
    </div>
  );
}

export function Shell() {
  const t = useT();
  const session = useSession();
  const panel = usePanel();
  const me = useQuery(meQuery(session.api));
  // The user's language as the server knows it (a choice made elsewhere, another device).
  useEffect(() => serverLocale(me.data?.locale, me.dataUpdatedAt), [me.data?.locale, me.dataUpdatedAt]);
  const brand = panel.data?.brand;
  const locale = useLocale();
  useEffect(() => {
    if (brand) document.title = `${brand} · ${t("shell.admin")}`;
  }, [brand, t, locale]);
  const pathname = useRouterState({ select: (state) => state.location.pathname });
  const [error, setError] = useState<Text | null>(null);
  const [leaving, setLeaving] = useState(false);
  const [collapsed, setCollapsed] = useState(false);
  const [drawer, setDrawer] = useState(false);

  useEffect(() => {
    const desktop = window.matchMedia("(min-width: 768px)");
    const closeOnDesktop = () => { if (desktop.matches) setDrawer(false); };
    desktop.addEventListener("change", closeOnDesktop);
    return () => desktop.removeEventListener("change", closeOnDesktop);
  }, []);
  useEffect(() => setDrawer(false), [pathname]);

  const logout = async () => {
    setError(null);
    setLeaving(true);
    try {
      await session.api.post("/auth/logout");
      session.signedOut();
    } catch (e) {
      // Already signed out on the server: the 401 handler takes over.
      if (!(e instanceof ApiError && e.status === 401)) setError(() => errorText(e, "nav.signOutFailed"));
    } finally {
      setLeaving(false);
    }
  };

  if (panel.isLoading) return <Loading />;
  // A failed background refresh keeps the shell (and the page inside it) mounted.
  if (!panel.data) return <ErrorState error={panel.error} />;
  const p = panel.data;
  const path = pathname.startsWith(session.config.basePath + "/") ? pathname.slice(session.config.basePath.length) : pathname;
  const resource = p.navigation.find((r) => path.startsWith(`/r/${r.slug}/`) || path === `/r/${r.slug}`);
  const settingsPage = (p.settings ?? []).find((s) => path === `/settings/${s.group}`);
  const current = resource?.pluralLabel ?? settingsPage?.label ?? (path.startsWith("/system/users") ? t("nav.users") : path.startsWith("/system/roles") ? t("nav.roles") : path.startsWith("/system/audit") ? t("nav.audit") : path.startsWith("/system/queue") ? t("nav.queue") : path.startsWith("/account") ? t("nav.account") : t("nav.overview"));
  const detail = /\/create$/.test(path) ? t("crumb.create") : /\/edit$/.test(path) ? t("crumb.edit") : resource && path.includes("/records/") ? t("crumb.details") : undefined;
  const footer = { panel: p, leaving, logout: () => void logout(), error: error === null ? null : show(error) };

  return (
    <div className="flex min-h-dvh">
      <a href="#main-content" onClick={(event) => { event.preventDefault(); document.getElementById("main-content")?.focus(); }} className="sr-only z-50 rounded-md bg-primary px-4 py-2 text-sm text-primary-foreground focus:not-sr-only focus:fixed focus:left-4 focus:top-4">{t("shell.skip")}</a>
      <aside className={cn("sticky top-0 hidden h-dvh shrink-0 flex-col gap-4 border-r border-border bg-background p-3 md:flex", collapsed ? "w-16" : "w-64")}>
        <Link to="/" className={cn("flex h-11 min-w-0 items-center gap-3 px-2 font-semibold tracking-tight", collapsed && "justify-center px-0")} title={p.brand}>
          <span className="flex size-8 shrink-0 items-center justify-center rounded-lg bg-primary text-primary-foreground"><PanelsTopLeft className="size-4" /></span><span className={cn("truncate", collapsed && "sr-only")}>{p.brand}</span>
        </Link>
        <Navigation panel={p} collapsed={collapsed} />
        <UserFooter {...footer} collapsed={collapsed} />
      </aside>
      <div className="flex min-w-0 flex-1 flex-col bg-card">
        <header className="sticky top-0 z-10 flex h-16 shrink-0 items-center gap-3 border-b border-border bg-card px-4 sm:px-6">
          <Button variant="ghost" size="icon" aria-label={collapsed ? t("shell.expand") : t("shell.collapse")} aria-expanded={!collapsed} className="hidden md:inline-flex" onClick={() => setCollapsed((value) => !value)}>{collapsed ? <PanelLeftOpen /> : <PanelLeftClose />}</Button>
          <Button variant="ghost" size="icon" aria-label={t("shell.openNav")} aria-haspopup="dialog" aria-expanded={drawer} className="md:hidden" onClick={() => setDrawer(true)}><Menu /></Button>
          <span className="h-5 w-px shrink-0 bg-border" aria-hidden="true" />
          <nav aria-label={t("shell.breadcrumb")} className="flex min-w-0 items-center gap-2 text-sm">
            <Link to="/" className="hidden text-muted-foreground hover:text-foreground sm:inline">{t("nav.workspace")}</Link>
            <ChevronRight className="hidden size-3 shrink-0 text-muted-foreground sm:block" />
            <span className="truncate font-medium" aria-current={detail ? undefined : "page"}>{current}</span>
            {detail && <><ChevronRight className="size-3 shrink-0 text-muted-foreground" /><span className="text-muted-foreground" aria-current="page">{detail}</span></>}
          </nav>
          <span className="ml-auto hidden shrink-0 rounded-md border border-border px-2 py-1 text-xs text-muted-foreground sm:inline">{t("shell.admin")}</span>
        </header>
        <main id="main-content" tabIndex={-1} className="min-w-0 flex-1 p-4 outline-none sm:p-6 lg:p-8"><Outlet /></main>
      </div>
      <Dialog open={drawer} onClose={() => setDrawer(false)} title={t("shell.navigation")} className="admin-drawer md:hidden">
        <Button variant="ghost" size="icon" aria-label={t("shell.closeNav")} className="absolute right-3 top-3" onClick={() => setDrawer(false)}><X /></Button>
        <div className="flex min-h-0 flex-1 flex-col gap-4">
          <Link to="/" className="flex items-center gap-2 font-semibold" onClick={() => setDrawer(false)}><PanelsTopLeft className="size-5" /><span className="truncate">{p.brand}</span></Link>
          <Navigation panel={p} onNavigate={() => setDrawer(false)} />
          <UserFooter {...footer} onNavigate={() => setDrawer(false)} />
        </div>
      </Dialog>
    </div>
  );
}

export function Dashboard() {
  const t = useT();
  const panel = usePanel();
  const p = panel.data;
  return (
    <Page>
      <PageHeader title={p ? t("dashboard.welcomeName", { name: p.user.name }) : t("dashboard.welcome")} description={t("dashboard.description")} eyebrow={t("nav.overview")} />
      <section className="mt-2">
        <div className="mb-4 flex items-center gap-3"><h2 className="text-sm font-semibold">{t("dashboard.resources")}</h2><span className="h-px flex-1 bg-border" /></div>
        {p?.navigation.length ? (
          <div className={cn("grid gap-4 sm:grid-cols-2", p.navigation.length > 2 && "xl:grid-cols-3")}>
            {p.navigation.map((r) => (
              <Link key={r.slug} to="/r/$slug" params={{ slug: r.slug }} className="group flex min-w-0 flex-col gap-5 rounded-xl border border-border bg-card p-5 shadow-xs transition-colors hover:bg-muted/40 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring">
                <div className="flex items-center justify-between"><span className="flex size-10 items-center justify-center rounded-lg border border-border bg-background"><ResourceIcon name={r.icon} /></span><ArrowUpRight className="size-4 text-muted-foreground group-hover:text-foreground" /></div>
                <div className="space-y-1"><h3 className="break-words text-base font-semibold tracking-tight">{r.pluralLabel}</h3><p className="text-sm text-muted-foreground">{t("dashboard.manage", { resource: r.pluralLabel.toLocaleLowerCase() })}</p></div>
              </Link>
            ))}
          </div>
        ) : <div className="rounded-xl border border-dashed border-border p-8 text-center text-sm text-muted-foreground">{t("dashboard.empty")}</div>}
      </section>
      <section className="rounded-xl border border-border bg-background p-5 sm:p-6">
        <h2 className="mb-1 text-sm font-semibold">{t("dashboard.settings")}</h2>
        <p className="mb-4 text-sm text-muted-foreground">{p?.system?.users || p?.system?.roles ? t("dashboard.settingsAdmin") : t("dashboard.settingsOwn")}</p>
        <div className="flex flex-wrap gap-3">
          <Link to="/account" className={cn(navLink, "border border-border bg-card text-foreground")}><Settings2 className="size-4" />{t("dashboard.myAccount")}<ChevronRight className="size-3" /></Link>
          {p?.system?.users && <Link to="/system/users" className={cn(navLink, "border border-border bg-card text-foreground")}><ResourceIcon name="users" />{t("nav.users")}<ChevronRight className="size-3" /></Link>}
          {p?.system?.roles && <Link to="/system/roles" className={cn(navLink, "border border-border bg-card text-foreground")}><ResourceIcon name="shield" />{t("nav.roles")}<ChevronRight className="size-3" /></Link>}
        </div>
      </section>
    </Page>
  );
}
