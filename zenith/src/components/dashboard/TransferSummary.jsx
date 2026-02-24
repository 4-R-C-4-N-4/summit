import { useDaemon } from '../../hooks/useDaemon';
import { useUIState } from '../../hooks/useUIState';
import PulseDot from '../common/PulseDot';

export default function TransferSummary() {
  const { files } = useDaemon();
  const { setActiveView } = useUIState();
  const received = files?.received || [];
  const inProgress = files?.in_progress || [];

  return (
    <button
      onClick={() => setActiveView('files')}
      className="group bg-summit-raised/60 border border-summit-border hover:border-summit-border-bright rounded-xl p-4 text-left transition-all duration-150 cursor-pointer"
    >
      <div className="flex items-center justify-between mb-3">
        <span className="text-[10px] font-bold text-summit-accent tracking-[0.1em] uppercase">
          Files
        </span>
        <span className="text-[9px] text-white/15 group-hover:text-white/25 transition-colors">→</span>
      </div>

      {inProgress.length > 0 ? (
        <div className="flex flex-col gap-1.5">
          {inProgress.slice(0, 2).map((f, i) => (
            <div key={i} className="flex items-center gap-2 text-[10px] text-summit-accent">
              <PulseDot color="#58a6ff" size={5} />
              <span className="truncate">{f}</span>
            </div>
          ))}
          {inProgress.length > 2 && (
            <span className="text-[9px] text-white/20">+{inProgress.length - 2} more</span>
          )}
        </div>
      ) : received.length > 0 ? (
        <div className="text-lg font-bold text-summit-cream tabular-nums">
          {received.length}
          <span className="text-[10px] font-normal text-white/25 ml-1.5">
            file{received.length !== 1 ? 's' : ''} received
          </span>
        </div>
      ) : (
        <div className="text-[11px] text-white/20">No transfers</div>
      )}
    </button>
  );
}
