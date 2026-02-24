import { useDaemon } from '../../hooks/useDaemon';
import { useUIState } from '../../hooks/useUIState';

export default function ComputeSummary() {
  const { computeTasks } = useDaemon();
  const { setActiveView } = useUIState();
  const tasks = computeTasks || [];
  const s = (t) => (t.status || '').toLowerCase();
  const active = tasks.filter(t => s(t) === 'running' || s(t) === 'in_progress').length;
  const queued = tasks.filter(t => s(t) === 'queued' || s(t) === 'pending').length;
  const completed = tasks.filter(t => s(t) === 'completed' || s(t) === 'done').length;
  const total = tasks.length;

  return (
    <button
      onClick={() => setActiveView('compute')}
      className="group bg-summit-raised/60 border border-summit-border hover:border-summit-border-bright rounded-xl p-4 text-left transition-all duration-150 cursor-pointer"
    >
      <div className="flex items-center justify-between mb-3">
        <span className="text-[10px] font-bold text-summit-accent tracking-[0.1em] uppercase">
          Compute
        </span>
        <span className="text-[9px] text-white/15 group-hover:text-white/25 transition-colors">→</span>
      </div>
      {total === 0 ? (
        <div className="text-[11px] text-white/20">No tasks submitted</div>
      ) : (
        <div className="grid grid-cols-3 gap-2">
          <Stat label="Active" value={active} color="text-summit-blue" />
          <Stat label="Queued" value={queued} color="text-summit-purple" />
          <Stat label="Done" value={completed} color="text-summit-green" />
        </div>
      )}
    </button>
  );
}

function Stat({ label, value, color }) {
  return (
    <div>
      <div className={`text-lg font-bold tabular-nums ${color}`}>{value}</div>
      <div className="text-[8px] text-white/25 tracking-wider uppercase">{label}</div>
    </div>
  );
}
