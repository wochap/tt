import { createContext, useContext } from "react";

export interface SessionControl {
  /** Revokes the token, wipes local documents and caches, returns to login. */
  logout: () => Promise<void>;
}

export const SessionControlContext = createContext<SessionControl | null>(null);

export function useSessionControl(): SessionControl {
  const control = useContext(SessionControlContext);
  if (!control) throw new Error("useSessionControl outside its provider");
  return control;
}
