import { createContext, useCallback, useContext, useEffect, useState, type ReactNode } from "react";
import { api, IN_TAURI, onEvent } from "./api";
import { formatTokenCount, isTokenUnit, type TokenUnit } from "./tokenFormat";

const TokenUnits = createContext<{
  unit: TokenUnit;
  ready: boolean;
  setUnit: (unit: TokenUnit) => Promise<void>;
}>({ unit: "chinese", ready: false, setUnit: async () => {} });

export function TokenUnitProvider({ children }: { children: ReactNode }) {
  const [unit, setUnitState] = useState<TokenUnit>("chinese");
  const [ready, setReady] = useState(false);
  useEffect(() => {
    let cancelled = false;
    let heardEvent = false;
    const storage = (event: StorageEvent) => {
      if (!IN_TAURI && event.key === "tokbar-token-unit" && isTokenUnit(event.newValue)) {
        heardEvent = true;
        setUnitState(event.newValue);
      }
    };
    window.addEventListener("storage", storage);
    const unlisten = onEvent<TokenUnit>("token-unit-changed", ({ payload }) => {
      if (!isTokenUnit(payload)) return;
      heardEvent = true;
      setUnitState(payload);
    });
    // Register first so a change during startup cannot be overwritten by a stale read.
    unlisten.then(() => api.getTokenUnit()).then((saved) => {
      if (!cancelled && !heardEvent && isTokenUnit(saved)) setUnitState(saved);
    }).catch((error) => console.error("get_token_unit failed:", error))
      .finally(() => { if (!cancelled) setReady(true); });
    return () => { cancelled = true; window.removeEventListener("storage", storage); unlisten.then((off) => off()); };
  }, []);
  const setUnit = useCallback(async (next: TokenUnit) => {
    await api.setTokenUnit(next);
    setUnitState(next);
  }, []);
  return <TokenUnits.Provider value={{ unit, ready, setUnit }}>{children}</TokenUnits.Provider>;
}

export function useTokenUnits() { return useContext(TokenUnits); }

export function useTokenFormatter() {
  const { unit } = useTokenUnits();
  return useCallback((value: number) => formatTokenCount(value, unit), [unit]);
}
