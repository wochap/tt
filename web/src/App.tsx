import { useCallback, useEffect, useMemo, useState } from "react";
import { createBrowserRouter, Navigate, RouterProvider } from "react-router";

import { ClientProvider, SessionProvider } from "@/data/react";
import { SessionControlContext } from "@/data/session-control";
import { clearCaches } from "@/lib/pwa";
import { useThemeSync } from "@/lib/theme";
import { clearAuth, loadAuth, onAuthChange, revoke, saveAuth } from "@/sync/auth";
import { SyncClient } from "@/sync/client";
import type { Auth } from "@/sync/protocol";

import { LoginPage } from "./features/login/LoginPage.tsx";
import { ReportsPage } from "./features/reports/ReportsPage.tsx";
import { SettingsPage } from "./features/settings/SettingsPage.tsx";
import { AppShell } from "./features/shell/AppShell.tsx";
import { TaskDetailPage } from "./features/tasks/TaskDetailPage.tsx";
import { TasksPage } from "./features/tasks/TasksPage.tsx";
import { TimelinePage } from "./features/timeline/TimelinePage.tsx";

const router = createBrowserRouter([
  {
    path: "/",
    element: <AppShell />,
    children: [
      { index: true, element: <Navigate to="/timeline/day" replace /> },
      { path: "timeline", element: <Navigate to="/timeline/day" replace /> },
      { path: "timeline/:range", element: <TimelinePage /> },
      { path: "tasks", element: <TasksPage /> },
      { path: "tasks/:seq", element: <TaskDetailPage /> },
      { path: "reports", element: <ReportsPage /> },
      { path: "settings", element: <SettingsPage /> },
      { path: "*", element: <Navigate to="/timeline/day" replace /> },
    ],
  },
]);

export function App() {
  useThemeSync();
  const [client, setClient] = useState<SyncClient>();
  const [auth, setAuth] = useState<Auth | null>(loadAuth);

  useEffect(() => {
    void SyncClient.start(loadAuth()).then(setClient);
  }, []);

  // Sign-in or sign-out in another tab: follow it.
  useEffect(() => onAuthChange(() => location.reload()), []);

  const onLogin = useCallback(
    (next: Auth, persist: boolean) => {
      saveAuth(next, persist);
      client?.setAuth(next);
      setAuth(next);
      if (location.pathname === "/login") history.replaceState(null, "", "/");
    },
    [client],
  );

  const control = useMemo(
    () => ({
      logout: async () => {
        const current = auth;
        clearAuth();
        if (current) await revoke(current);
        await client?.wipe().catch((error) => console.error("wipe failed", error));
        await clearCaches();
        location.replace("/");
      },
    }),
    [auth, client],
  );

  if (!client) {
    return (
      <div className="flex h-full items-center justify-center text-[12px] text-muted" role="status">
        Starting…
      </div>
    );
  }
  if (!auth) return <LoginPage onLogin={onLogin} />;
  return (
    <ClientProvider client={client}>
      <SessionControlContext.Provider value={control}>
        <SessionProvider client={client} auth={auth}>
          <RouterProvider router={router} />
        </SessionProvider>
      </SessionControlContext.Provider>
    </ClientProvider>
  );
}
