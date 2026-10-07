import { createContext, type ReactNode, useContext, useEffect, useState, useSyncExternalStore } from "react";

import type { Auth, SyncStatus } from "@/sync/protocol";
import type { SyncClient } from "@/sync/client";

import { type Snapshot, TtStore } from "./store.ts";

interface Session {
  client: SyncClient;
  auth: Auth;
  store: TtStore;
}

export const SessionContext = createContext<Session | null>(null);
const ClientContext = createContext<SyncClient | null>(null);

export function ClientProvider({ client, children }: { client: SyncClient; children: ReactNode }) {
  return <ClientContext.Provider value={client}>{children}</ClientContext.Provider>;
}

export function useClient(): SyncClient {
  const client = useContext(ClientContext);
  if (!client) throw new Error("useClient outside ClientProvider");
  return client;
}

export function SessionProvider({ client, auth, children }: { client: SyncClient; auth: Auth; children: ReactNode }) {
  const [store] = useState(() => new TtStore(client.repo, auth.indexDoc));
  useEffect(() => {
    void store.start();
    return () => store.dispose();
  }, [store]);
  return <SessionContext.Provider value={{ client, auth, store }}>{children}</SessionContext.Provider>;
}

export function useSession(): Session {
  const session = useContext(SessionContext);
  if (!session) throw new Error("useSession outside SessionProvider");
  return session;
}

export function useStore(): TtStore {
  return useSession().store;
}

export function useSnapshot(): Snapshot {
  const store = useStore();
  return useSyncExternalStore(store.subscribe, store.getSnapshot, store.getSnapshot);
}

export function useView() {
  return useSnapshot().view;
}

export function useSyncStatus(): SyncStatus {
  const client = useClient();
  return useSyncExternalStore(client.subscribe, client.getStatus, client.getStatus);
}

/** Current time, re-rendering every `intervalMs` (aligned to the interval). */
export function useNow(intervalMs = 1000): number {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    let timer: ReturnType<typeof setTimeout>;
    const tick = () => {
      const at = Date.now();
      setNow(at);
      timer = setTimeout(tick, intervalMs - (at % intervalMs) + 5);
    };
    timer = setTimeout(tick, intervalMs - (Date.now() % intervalMs) + 5);
    return () => clearTimeout(timer);
  }, [intervalMs]);
  return now;
}
