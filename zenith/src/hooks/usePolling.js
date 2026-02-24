import { useState, useEffect, useRef, useCallback } from 'react';

export function usePolling(fetchFn, interval = 2000) {
  const [data, setData] = useState(null);
  const [error, setError] = useState(null);
  const savedFn = useRef(fetchFn);

  useEffect(() => { savedFn.current = fetchFn; }, [fetchFn]);

  const refresh = useCallback(async () => {
    try {
      const result = await savedFn.current();
      setData(result);
      setError(null);
      return result;
    } catch (err) {
      setError(err);
      throw err;
    }
  }, []);

  useEffect(() => {
    refresh();
    const id = setInterval(refresh, interval);
    return () => clearInterval(id);
  }, [refresh, interval]);

  return { data, error, refresh };
}
