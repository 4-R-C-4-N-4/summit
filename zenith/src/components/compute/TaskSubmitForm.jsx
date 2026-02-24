import { useState } from 'react';
import { useDaemon } from '../../hooks/useDaemon';
import PeerSelector from './PeerSelector';

const MODES = [
  { id: 'shell', label: 'Shell', desc: 'Run a shell command' },
  { id: 'exec', label: 'Execute', desc: 'Run a binary with args' },
  { id: 'json', label: 'Raw JSON', desc: 'Custom payload' },
];

function buildPayload(mode, fields) {
  switch (mode) {
    case 'shell':
      return { run: fields.command };
    case 'exec': {
      const args = fields.args
        .split('\n')
        .map(a => a.trim())
        .filter(Boolean);
      return { cmd: fields.cmd, args };
    }
    case 'json':
      return JSON.parse(fields.json);
    default:
      return {};
  }
}

export default function TaskSubmitForm() {
  const { submitComputeTask } = useDaemon();
  const [peer, setPeer] = useState(null);
  const [mode, setMode] = useState('shell');
  const [fields, setFields] = useState({
    command: '',
    cmd: '',
    args: '',
    json: '{\n  \n}',
  });
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState(null);

  const setField = (key, value) => setFields(prev => ({ ...prev, [key]: value }));

  const canSubmit = () => {
    if (!peer) return false;
    if (mode === 'shell') return fields.command.trim().length > 0;
    if (mode === 'exec') return fields.cmd.trim().length > 0;
    if (mode === 'json') {
      try { JSON.parse(fields.json); return true; } catch { return false; }
    }
    return false;
  };

  const handleSubmit = async () => {
    if (!canSubmit()) return;
    setSubmitting(true);
    setError(null);
    try {
      const payload = buildPayload(mode, fields);
      await submitComputeTask(peer, payload);
      // Reset fields on success
      setFields(prev => ({ ...prev, command: '', cmd: '', args: '', json: '{\n  \n}' }));
    } catch (err) {
      setError(err.message);
    } finally {
      setSubmitting(false);
    }
  };

  return (
    <div className="bg-summit-raised/60 border border-summit-border rounded-xl p-5 flex flex-col gap-4">
      <PeerSelector value={peer} onChange={setPeer} />

      {/* Mode tabs */}
      <div className="flex flex-col gap-1.5">
        <label className="text-[10px] text-white/35 tracking-[0.1em] uppercase font-bold">
          Task Type
        </label>
        <div className="flex gap-1.5">
          {MODES.map(m => (
            <button
              key={m.id}
              type="button"
              onClick={() => setMode(m.id)}
              className={`
                flex-1 px-3 py-2 rounded-lg text-left transition-all duration-100 cursor-pointer border
                ${mode === m.id
                  ? 'bg-summit-accent/10 border-summit-accent/20 text-summit-accent'
                  : 'bg-white/[0.02] border-white/6 text-white/30 hover:text-white/45 hover:border-white/10'}
              `}
            >
              <div className="text-[11px] font-bold">{m.label}</div>
              <div className="text-[8px] text-inherit opacity-50 mt-0.5">{m.desc}</div>
            </button>
          ))}
        </div>
      </div>

      {/* Mode-specific fields */}
      {mode === 'shell' && (
        <ShellFields command={fields.command} onChange={(v) => setField('command', v)} />
      )}
      {mode === 'exec' && (
        <ExecFields cmd={fields.cmd} args={fields.args} onCmdChange={(v) => setField('cmd', v)} onArgsChange={(v) => setField('args', v)} />
      )}
      {mode === 'json' && (
        <JsonFields json={fields.json} onChange={(v) => setField('json', v)} />
      )}

      {/* Payload preview */}
      {mode !== 'json' && canSubmit() && (
        <PayloadPreview mode={mode} fields={fields} />
      )}

      {error && (
        <div className="text-[10px] text-summit-red bg-summit-red/8 border border-summit-red/15 rounded-lg px-3 py-2">
          {error}
        </div>
      )}

      <button
        onClick={handleSubmit}
        disabled={!canSubmit() || submitting}
        className={`
          px-4 py-2.5 rounded-lg text-[11px] font-bold tracking-[0.06em] uppercase transition-all duration-150 cursor-pointer
          ${canSubmit() && !submitting
            ? 'bg-summit-accent/15 border border-summit-accent/25 text-summit-accent hover:bg-summit-accent/20 active:scale-[0.98]'
            : 'bg-white/[0.03] border border-white/8 text-white/20 cursor-not-allowed'}
        `}
      >
        {submitting ? 'Submitting...' : 'Submit Task'}
      </button>
    </div>
  );
}

function ShellFields({ command, onChange }) {
  return (
    <div className="flex flex-col gap-1.5">
      <label className="text-[10px] text-white/35 tracking-[0.1em] uppercase font-bold">
        Command
      </label>
      <input
        type="text"
        value={command}
        onChange={(e) => onChange(e.target.value)}
        placeholder="e.g. ls -la /tmp && df -h"
        spellCheck={false}
        className="bg-summit-bg border border-white/8 rounded-lg px-3 py-2.5 text-[11px] text-summit-cream outline-none focus:border-summit-accent/30 focus:ring-1 focus:ring-summit-accent/10 transition-all placeholder:text-white/12"
      />
      <span className="text-[9px] text-white/15">
        Runs via <code className="text-summit-accent/50 bg-summit-accent/5 px-1 rounded">sh -c</code> — supports pipes, redirections, chaining
      </span>
    </div>
  );
}

function ExecFields({ cmd, args, onCmdChange, onArgsChange }) {
  return (
    <div className="flex flex-col gap-3">
      <div className="flex flex-col gap-1.5">
        <label className="text-[10px] text-white/35 tracking-[0.1em] uppercase font-bold">
          Executable
        </label>
        <input
          type="text"
          value={cmd}
          onChange={(e) => onCmdChange(e.target.value)}
          placeholder="e.g. python3"
          spellCheck={false}
          className="bg-summit-bg border border-white/8 rounded-lg px-3 py-2.5 text-[11px] text-summit-cream outline-none focus:border-summit-accent/30 focus:ring-1 focus:ring-summit-accent/10 transition-all placeholder:text-white/12"
        />
      </div>
      <div className="flex flex-col gap-1.5">
        <label className="text-[10px] text-white/35 tracking-[0.1em] uppercase font-bold">
          Arguments <span className="text-white/15 font-normal">(one per line)</span>
        </label>
        <textarea
          value={args}
          onChange={(e) => onArgsChange(e.target.value)}
          placeholder={"-c\nprint('hello')"}
          rows={3}
          spellCheck={false}
          className="bg-summit-bg border border-white/8 rounded-lg px-3 py-2.5 text-[11px] text-summit-cream outline-none focus:border-summit-accent/30 focus:ring-1 focus:ring-summit-accent/10 resize-y transition-all placeholder:text-white/12"
        />
      </div>
    </div>
  );
}

function JsonFields({ json, onChange }) {
  let valid = true;
  try { JSON.parse(json); } catch { valid = false; }

  return (
    <div className="flex flex-col gap-1.5">
      <div className="flex items-center justify-between">
        <label className="text-[10px] text-white/35 tracking-[0.1em] uppercase font-bold">
          Payload
        </label>
        {json.trim() && (
          <span className={`text-[9px] ${valid ? 'text-summit-green/50' : 'text-summit-red/50'}`}>
            {valid ? 'valid JSON' : 'invalid JSON'}
          </span>
        )}
      </div>
      <textarea
        value={json}
        onChange={(e) => onChange(e.target.value)}
        rows={6}
        spellCheck={false}
        className={`
          bg-summit-bg border rounded-lg px-3 py-2.5 text-[11px] text-summit-cream outline-none resize-y transition-all
          ${valid ? 'border-white/8 focus:border-summit-accent/30 focus:ring-1 focus:ring-summit-accent/10' : 'border-summit-red/20 focus:border-summit-red/30 focus:ring-1 focus:ring-summit-red/10'}
        `}
      />
    </div>
  );
}

function PayloadPreview({ mode, fields }) {
  let payload;
  try { payload = buildPayload(mode, fields); } catch { return null; }

  return (
    <details className="group">
      <summary className="text-[9px] text-white/20 cursor-pointer hover:text-white/30 transition-colors select-none list-none flex items-center gap-1.5">
        <svg width="10" height="10" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="2" className="transition-transform group-open:rotate-90">
          <path d="M6 4l4 4-4 4" />
        </svg>
        Preview payload
      </summary>
      <pre className="mt-1.5 bg-summit-bg border border-summit-border rounded-lg px-3 py-2 text-[10px] text-white/30 overflow-auto max-h-24">
        {JSON.stringify(payload, null, 2)}
      </pre>
    </details>
  );
}
