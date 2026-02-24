import { useState, useEffect, useCallback } from 'react';

const isElectron = typeof window !== 'undefined' && !!window.electron?.readConfig;

export function useConfig() {
  const [config, setConfig] = useState(null);
  const [configPath, setConfigPath] = useState(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState(null);
  const [saving, setSaving] = useState(false);

  const load = useCallback(async () => {
    if (!isElectron) { setLoading(false); return; }
    setLoading(true);
    const result = await window.electron.readConfig();
    if (result.ok) {
      setConfig(result.config);
      setConfigPath(result.path);
      setError(null);
    } else {
      setError(result.error);
    }
    setLoading(false);
  }, []);

  useEffect(() => { load(); }, [load]);

  const save = useCallback(async (updates) => {
    // updates: [{ section, key, value }, ...]
    if (!isElectron) return { ok: false, error: 'Not running in Electron' };
    setSaving(true);
    const result = await window.electron.writeConfig(updates);
    if (result.ok) await load();
    setSaving(false);
    return result;
  }, [load]);

  const openStoragePath = useCallback(async (p) => {
    if (!isElectron || !p) return;
    await window.electron.openPath(p);
  }, []);

  return { config, configPath, loading, error, saving, save, reload: load, openStoragePath };
}
