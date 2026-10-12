import { Link, Outlet } from "@tanstack/react-router";
import { LogOut, PanelsTopLeft } from "lucide-react";
import { useState } from "react";
import { ApiError } from "@/api/client";
import { useSession } from "@/api/context";
import { usePanel } from "@/api/queries";
import type { ResourceInfo } from "@/api/types";
import { ErrorState, Loading } from "@/components/states";
import { Alert } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import { ResourceIcon } from "@/components/icons";

const navLink = "flex items-center gap-2 rounded-md px-2 py-1.5 text-sm hover:bg-muted [&.active]:bg-muted [&.active]:font-medium";

function groups(navigation: ResourceInfo[]): [string, ResourceInfo[]][] {
  const out = new Map<string, ResourceInfo[]>();
  for (const r of navigation) {
    const g = r.group ?? "";
    out.set(g, [...(out.get(g) ?? []), r]);
  }
  return [...out.entries()];
}

export function Shell() {
  const session = useSession();
  const panel = usePanel();
  const [error, setError] = useState<string | null>(null);
  const [leaving, setLeaving] = useState(false);

  const logout = async () => {
    setError(null);
    setLeaving(true);
    try {
      await session.api.post("/auth/logout");
      session.signedOut();
    } catch (e) {
      // Already signed out on the server: the 401 handler takes over.
      if (!(e instanceof ApiError && e.status === 401)) setError(e instanceof Error ? e.message : "Sign-out failed.");
    } finally {
      setLeaving(false);
    }
  };

  if (panel.isLoading) return <Loading />;
  // A failed background refresh keeps the shell (and the page inside it) mounted.
  if (!panel.data) return <ErrorState error={panel.error} />;
  const p = panel.data;
  return (
    <div className="flex min-h-screen flex-col md:flex-row">
      <aside className="flex flex-col gap-4 border-b border-border bg-card p-4 md:w-60 md:border-b-0 md:border-r">
        <Link to="/" className="flex items-center gap-2 text-base font-semibold">
          <PanelsTopLeft className="size-5 text-primary" /> {p.brand}
        </Link>
        <nav aria-label="Resources" className="flex flex-col gap-3">
          {groups(p.navigation).map(([group, items]) => (
            <div key={group} className="flex flex-col gap-1">
              {group && <p className="px-2 text-xs font-semibold uppercase text-muted-foreground">{group}</p>}
              {items.map((r) => (
                <Link
                  key={r.slug}
                  to="/r/$slug"
                  params={{ slug: r.slug }}
                  className={navLink}
                >
                  <ResourceIcon name={r.icon} /> {r.pluralLabel}
                </Link>
              ))}
            </div>
          ))}
          {/* `system` is absent from older servers. */}
          {(p.system?.users || p.system?.roles) && (
            <div className="flex flex-col gap-1">
              <p className="px-2 text-xs font-semibold uppercase text-muted-foreground">System</p>
              {p.system?.users && (
                <Link to="/system/users" className={navLink}>
                  <ResourceIcon name="users" /> Users
                </Link>
              )}
              {p.system?.roles && (
                <Link to="/system/roles" className={navLink}>
                  <ResourceIcon name="shield" /> Roles
                </Link>
              )}
            </div>
          )}
        </nav>
        <div className="mt-auto flex flex-col gap-2 border-t border-border pt-3 text-sm">
          <Link to="/account" className="truncate hover:underline" title={p.user.email}>
            {p.user.name}
          </Link>
          <Button variant="outline" size="sm" onClick={() => void logout()} disabled={leaving}>
            <LogOut /> Sign out
          </Button>
          {error && <Alert variant="destructive">{error}</Alert>}
        </div>
      </aside>
      <main className="flex-1 p-4 md:p-8">
        <Outlet />
      </main>
    </div>
  );
}

export function Dashboard() {
  const panel = usePanel();
  return (
    <div className="flex flex-col gap-2">
      <h1 className="text-xl font-semibold">Welcome{panel.data ? `, ${panel.data.user.name}` : ""}</h1>
      <p className="text-sm text-muted-foreground">
        {panel.data?.navigation.length ? "Pick a resource on the left." : "There is nothing you can manage yet."}
      </p>
    </div>
  );
}
