import { useCallback, useEffect, useMemo, useState } from "react";
import { createBrowserRouter, Navigate, RouterProvider } from "react-router";

import { ClientProvider, SessionProvider } from "@/data/react";
import { SessionControlContext } from "@/data/session-control";
import { clearCaches } from "@/lib/pwa";
import { useThemeSync } from "@/lib/theme";
import { clearAuth, defaultServer, loadAuth, onAuthChange, refreshMe, revoke, saveAuth, updateAuth } from "@/sync/auth";
import { SyncClient } from "@/sync/client";
import type { Auth } from "@/sync/protocol";

import { LoginPage } from "./features/login/LoginPage.tsx";
import { ReportsPage } from "./features/reports/ReportsPage.tsx";
import { SettingsPage } from "./features/settings/SettingsPage.tsx";
import { SetupGate } from "./features/setup/SetupGate.tsx";
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

  // Refresh user and server names once per session; offline keeps the stored ones.
  const session = auth?.token;
  useEffect(() => {
    const current = loadAuth();
    if (!session || current?.token !== session) return;
    void refreshMe(current).then((next) => {
      if (!next || JSON.stringify(next) === JSON.stringify(current)) return;
      updateAuth(next);
      setAuth(next);
    });
  }, [session]);

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
  // A server that is not set up has nobody to sign in as: say so instead.
  if (!auth) {
    return (
      <SetupGate server={defaultServer()}>
        <LoginPage onLogin={onLogin} />
      </SetupGate>
    );
  }
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
