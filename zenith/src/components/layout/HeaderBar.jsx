import { useDaemon } from '../../hooks/useDaemon';
import PulseDot from '../common/PulseDot';

export default function HeaderBar() {
  const { connected, status, peers } = useDaemon();
  const sessionCount = status?.sessions?.length || 0;
  const peerCount = peers?.length || 0;

  return (
    <header className="h-12 px-5 flex items-center justify-between border-b border-summit-border bg-summit-surface/80 backdrop-blur-sm">
      <div className="flex items-center gap-3">
        <span className="text-[15px] font-bold text-summit-white tracking-[0.12em] uppercase">
          Zenith
        </span>
        <span className="text-[9px] text-white/20 tracking-[0.1em] uppercase hidden sm:inline">
          Summit Mesh
        </span>
      </div>

      <div className="flex items-center gap-5">
        <div className="flex items-center gap-4 text-[10px] text-white/35 tracking-wide">
          <span className="flex items-center gap-1.5">
            <span className="inline-block w-1 h-1 rounded-full bg-summit-accent/40" />
            {peerCount} peer{peerCount !== 1 ? 's' : ''}
          </span>
          <span className="flex items-center gap-1.5">
            <span className="inline-block w-1 h-1 rounded-full bg-summit-green/40" />
            {sessionCount} session{sessionCount !== 1 ? 's' : ''}
          </span>
        </div>

        <div className="flex items-center gap-1.5 pl-4 border-l border-summit-border">
          {connected ? (
            <>
              <PulseDot color="#3fb950" size={6} />
              <span className="text-[9px] text-summit-green tracking-wider uppercase">Live</span>
            </>
          ) : (
            <>
              <span className="w-1.5 h-1.5 rounded-full bg-summit-red" />
              <span className="text-[9px] text-summit-red tracking-wider uppercase">Offline</span>
            </>
          )}
        </div>
      </div>
    </header>
  );
}
