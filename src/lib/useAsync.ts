import { useEffect, useState } from "react";

export interface AsyncState<T> { data: T | null; error: string | null; loading: boolean }

/** Runs `fn` whenever `deps` change; keeps the previous data while reloading, but drops it on error. */
export function useAsync<T>(fn: () => Promise<T>, deps: unknown[]): AsyncState<T> {
  const [state, setState] = useState<AsyncState<T>>({ data: null, error: null, loading: true });
  useEffect(() => {
    let cancelled = false;
    setState((s) => ({ ...s, loading: true }));
    fn().then(
      (data) => { if (!cancelled) setState({ data, error: null, loading: false }); },
      (e) => { if (!cancelled) setState(() => ({ data: null, error: String(e), loading: false })); },
    );
    return () => { cancelled = true; };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, deps);
  return state;
}
