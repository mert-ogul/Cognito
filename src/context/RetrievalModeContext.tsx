import { createContext, useContext, useMemo, useState, type ReactNode } from "react";

type Ctx = {
  lastRetrievalMode: string | null;
  setLastRetrievalMode: (m: string | null) => void;
};

const RetrievalModeContext = createContext<Ctx | null>(null);

export function RetrievalModeProvider({ children }: { children: ReactNode }) {
  const [lastRetrievalMode, setLastRetrievalMode] = useState<string | null>(null);
  const v = useMemo(
    () => ({ lastRetrievalMode, setLastRetrievalMode }),
    [lastRetrievalMode],
  );
  return <RetrievalModeContext.Provider value={v}>{children}</RetrievalModeContext.Provider>;
}

export function useRetrievalMode() {
  const x = useContext(RetrievalModeContext);
  if (!x) {
    return { lastRetrievalMode: null as string | null, setLastRetrievalMode: () => {} };
  }
  return x;
}
