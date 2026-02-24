import { useDaemon } from '../../hooks/useDaemon';
import { useUIState } from '../../hooks/useUIState';
import { shortKey } from '../../lib/format';

export default function MessagesSummary() {
  const { peers, trust } = useDaemon();
  const { setActiveView } = useUIState();
  const trustMap = {};
  trust.forEach(t => { trustMap[t.public_key] = t.level; });
  const trustedPeers = peers.filter(p => trustMap[p.public_key] === 'Trusted');

  return (
    <button
      onClick={() => setActiveView('messages')}
      className="group bg-summit-raised/60 border border-summit-border hover:border-summit-border-bright rounded-xl p-4 text-left transition-all duration-150 cursor-pointer"
    >
      <div className="flex items-center justify-between mb-3">
        <span className="text-[10px] font-bold text-summit-accent tracking-[0.1em] uppercase">
          Messages
        </span>
        <span className="text-[9px] text-white/15 group-hover:text-white/25 transition-colors">→</span>
      </div>

      {trustedPeers.length > 0 ? (
        <>
          <div className="text-lg font-bold text-summit-cream tabular-nums">
            {trustedPeers.length}
            <span className="text-[10px] font-normal text-white/25 ml-1.5">
              peer{trustedPeers.length !== 1 ? 's' : ''} available
            </span>
          </div>
          <div className="flex gap-1 mt-2">
            {trustedPeers.slice(0, 3).map(p => (
              <span key={p.public_key} className="text-[9px] text-summit-green/70 bg-summit-green/8 px-1.5 py-0.5 rounded">
                {shortKey(p.public_key)}
              </span>
            ))}
            {trustedPeers.length > 3 && (
              <span className="text-[9px] text-white/20 px-1">+{trustedPeers.length - 3}</span>
            )}
          </div>
        </>
      ) : (
        <div className="text-[11px] text-white/20">No trusted peers</div>
      )}
    </button>
  );
}
