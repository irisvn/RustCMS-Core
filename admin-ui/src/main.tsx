import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { RouterProvider } from "@tanstack/react-router";
import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { ApiError } from "@/api/client";
import { SessionContext } from "@/api/context";
import { createAppRouter } from "@/app/router";
import { createSession } from "@/app/session";
import { readConfig } from "@/lib/config";
import "./index.css";

const config = readConfig();
const queryClient = new QueryClient({
  defaultOptions: {
    queries: {
      // Client errors (401/403/404/422) are answers, not glitches.
      retry: (count, error) => !(error instanceof ApiError && error.status > 0 && error.status < 500) && count < 2,
      refetchOnWindowFocus: true,
      staleTime: 10_000,
    },
    // Writes are never retried: a failed request may still have been applied.
    mutations: { retry: false },
  },
});

const session = createSession(config, queryClient, (redirect) => {
  void router.navigate({ to: "/login", search: redirect ? { redirect } : {}, replace: true });
});
const router = createAppRouter({ queryClient, session });

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <SessionContext.Provider value={session}>
      <QueryClientProvider client={queryClient}>
        <RouterProvider router={router} />
      </QueryClientProvider>
    </SessionContext.Provider>
  </StrictMode>,
);
