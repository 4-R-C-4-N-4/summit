import { useState, useEffect } from 'react';
import { useConfig } from '../../hooks/useConfig';
import SectionHeader from '../common/SectionHeader';

export default function SettingsPanel() {
  const { config, configPath, loading, error, saving, save } = useConfig();
  const [draft, setDraft] = useState(null);
  const [saved, setSaved] = useState(false);

  // Initialise draft from loaded config
  useEffect(() => {
    if (!config) return;
    setDraft({
      network_interface: config.network?.interface ?? '',
      auto_trust: config.trust?.auto_trust ?? true,
      file_transfer: config.services?.file_transfer ?? true,
      messaging: config.services?.messaging ?? true,
      compute: config.services?.compute ?? true,
      ft_storage_path: config.services?.file_transfer_settings?.storage_path ?? '',
      ft_cache_max_bytes: config.services?.file_transfer_settings?.cache_max_bytes ?? 1073741824,
      msg_storage_path: config.services?.messaging_settings?.storage_path ?? '',
      msg_retention_days: config.services?.messaging_settings?.retention_days ?? 30,
      compute_work_dir: config.services?.compute_settings?.work_dir ?? '/tmp/summit-compute',
      compute_max_tasks: config.services?.compute_settings?.max_concurrent_tasks ?? 0,
      compute_max_cores: config.services?.compute_settings?.max_cpu_cores ?? 0,
    });
  }, [config]);

  const set = (key, value) => setDraft(prev => ({ ...prev, [key]: value }));

  const handleSave = async () => {
    const updates = [
      { section: 'network',                          key: 'interface',           value: draft.network_interface },
      { section: 'trust',                            key: 'auto_trust',          value: draft.auto_trust },
      { section: 'services',                         key: 'file_transfer',       value: draft.file_transfer },
      { section: 'services',                         key: 'messaging',           value: draft.messaging },
      { section: 'services',                         key: 'compute',             value: draft.compute },
      { section: 'services.file_transfer_settings',  key: 'storage_path',        value: draft.ft_storage_path },
      { section: 'services.file_transfer_settings',  key: 'cache_max_bytes',     value: Number(draft.ft_cache_max_bytes) },
      { section: 'services.messaging_settings',      key: 'storage_path',        value: draft.msg_storage_path },
      { section: 'services.messaging_settings',      key: 'retention_days',      value: Number(draft.msg_retention_days) },
      { section: 'services.compute_settings',        key: 'work_dir',            value: draft.compute_work_dir },
      { section: 'services.compute_settings',        key: 'max_concurrent_tasks', value: Number(draft.compute_max_tasks) },
      { section: 'services.compute_settings',        key: 'max_cpu_cores',       value: Number(draft.compute_max_cores) },
    ];
    const result = await save(updates);
    if (result.ok) {
      setSaved(true);
      setTimeout(() => setSaved(false), 2000);
    }
  };

  if (loading) return (
    <div className="text-[10px] text-white/20 py-4">Loading config...</div>
  );

  if (error) return (
    <div className="bg-summit-red/8 border border-summit-red/15 rounded-xl p-4 text-[10px] text-summit-red">
      Could not read config: {error}
    </div>
  );

  if (!draft) return null;

  return (
    <div className="flex flex-col gap-5">
      <SectionHeader label="Settings" />

      {configPath && (
        <div className="text-[9px] text-white/15 font-mono">{configPath}</div>
      )}

      {/* Network */}
      <SettingsGroup label="Network">
        <Field label="Interface" hint="Leave blank for auto-detect">
          <TextInput value={draft.network_interface} onChange={v => set('network_interface', v)} placeholder="e.g. wlan0" />
        </Field>
      </SettingsGroup>

      {/* Trust */}
      <SettingsGroup label="Trust">
        <Field label="Auto-trust new peers" hint="Automatically trust peers when discovered">
          <Toggle value={draft.auto_trust} onChange={v => set('auto_trust', v)} />
        </Field>
      </SettingsGroup>

      {/* Services */}
      <SettingsGroup label="Services">
        <Field label="File Transfer">
          <Toggle value={draft.file_transfer} onChange={v => set('file_transfer', v)} />
        </Field>
        <Field label="Messaging">
          <Toggle value={draft.messaging} onChange={v => set('messaging', v)} />
        </Field>
        <Field label="Compute">
          <Toggle value={draft.compute} onChange={v => set('compute', v)} />
        </Field>
      </SettingsGroup>

      {/* File transfer settings */}
      {draft.file_transfer && (
        <SettingsGroup label="File Transfer Settings">
          <Field label="Storage path">
            <TextInput value={draft.ft_storage_path} onChange={v => set('ft_storage_path', v)} placeholder="/home/user/.local/share/summit/files" />
          </Field>
          <Field label="Cache limit" hint="Bytes (0 = unlimited)">
            <NumberInput value={draft.ft_cache_max_bytes} onChange={v => set('ft_cache_max_bytes', v)} />
          </Field>
        </SettingsGroup>
      )}

      {/* Messaging settings */}
      {draft.messaging && (
        <SettingsGroup label="Messaging Settings">
          <Field label="Storage path">
            <TextInput value={draft.msg_storage_path} onChange={v => set('msg_storage_path', v)} placeholder="/home/user/.local/share/summit/messages" />
          </Field>
          <Field label="Retention" hint="Days to keep messages">
            <NumberInput value={draft.msg_retention_days} onChange={v => set('msg_retention_days', v)} min={1} />
          </Field>
        </SettingsGroup>
      )}

      {/* Compute settings */}
      {draft.compute && (
        <SettingsGroup label="Compute Settings">
          <Field label="Work directory">
            <TextInput value={draft.compute_work_dir} onChange={v => set('compute_work_dir', v)} placeholder="/tmp/summit-compute" />
          </Field>
          <Field label="Max concurrent tasks" hint="0 = CPU count">
            <NumberInput value={draft.compute_max_tasks} onChange={v => set('compute_max_tasks', v)} min={0} />
          </Field>
          <Field label="Max CPU cores" hint="0 = all cores">
            <NumberInput value={draft.compute_max_cores} onChange={v => set('compute_max_cores', v)} min={0} />
          </Field>
        </SettingsGroup>
      )}

      <button
        onClick={handleSave}
        disabled={saving}
        className={`
          w-full py-2.5 rounded-xl text-[11px] font-bold tracking-[0.06em] uppercase transition-all duration-150 cursor-pointer
          ${saved
            ? 'bg-summit-green/15 border border-summit-green/25 text-summit-green'
            : saving
              ? 'bg-white/[0.03] border border-white/8 text-white/20 cursor-not-allowed'
              : 'bg-summit-accent/15 border border-summit-accent/25 text-summit-accent hover:bg-summit-accent/20 active:scale-[0.99]'}
        `}
      >
        {saved ? 'Saved' : saving ? 'Saving...' : 'Save Settings'}
      </button>

      <p className="text-[9px] text-white/15 text-center">
        Changes take effect after restarting the daemon
      </p>
    </div>
  );
}

function SettingsGroup({ label, children }) {
  return (
    <div className="bg-summit-raised/40 border border-summit-border rounded-xl overflow-hidden">
      <div className="px-4 py-2 border-b border-summit-border">
        <span className="text-[9px] font-bold text-white/25 tracking-[0.1em] uppercase">{label}</span>
      </div>
      <div className="divide-y divide-summit-border">
        {children}
      </div>
    </div>
  );
}

function Field({ label, hint, children }) {
  return (
    <div className="px-4 py-3 flex items-center justify-between gap-4">
      <div className="min-w-0">
        <div className="text-[11px] text-summit-cream">{label}</div>
        {hint && <div className="text-[9px] text-white/20 mt-0.5">{hint}</div>}
      </div>
      <div className="shrink-0">{children}</div>
    </div>
  );
}

function Toggle({ value, onChange }) {
  return (
    <button
      type="button"
      onClick={() => onChange(!value)}
      className={`
        relative w-9 h-5 rounded-full border transition-all duration-200 cursor-pointer
        ${value
          ? 'bg-summit-accent/20 border-summit-accent/30'
          : 'bg-white/[0.04] border-white/10'}
      `}
    >
      <span className={`
        absolute top-0.5 w-4 h-4 rounded-full transition-all duration-200
        ${value
          ? 'left-[18px] bg-summit-accent'
          : 'left-0.5 bg-white/25'}
      `} />
    </button>
  );
}

function TextInput({ value, onChange, placeholder }) {
  return (
    <input
      type="text"
      value={value}
      onChange={e => onChange(e.target.value)}
      placeholder={placeholder}
      className="w-56 bg-summit-bg border border-white/8 rounded-lg px-3 py-1.5 text-[11px] text-summit-cream outline-none focus:border-summit-accent/30 focus:ring-1 focus:ring-summit-accent/10 transition-all placeholder:text-white/12"
      spellCheck={false}
    />
  );
}

function NumberInput({ value, onChange, min }) {
  return (
    <input
      type="number"
      value={value}
      min={min}
      onChange={e => onChange(e.target.value)}
      className="w-28 bg-summit-bg border border-white/8 rounded-lg px-3 py-1.5 text-[11px] text-summit-cream outline-none focus:border-summit-accent/30 focus:ring-1 focus:ring-summit-accent/10 transition-all text-right tabular-nums"
    />
  );
}
