import { useDaemon } from '../../hooks/useDaemon';
import { shortKey } from '../../lib/format';
import PulseDot from '../common/PulseDot';

export default function ConversationList({ selected, onSelect }) {
  const { peers, trust } = useDaemon();
  const trustMap = {};
  trust.forEach(t => { trustMap[t.public_key] = t.level; });
  const trustedPeers = peers.filter(p => trustMap[p.public_key] === 'Trusted');

  return (
    <div className="w-56 border-r border-summit-border bg-summit-surface overflow-y-auto flex flex-col">
      <div className="px-3 pt-4 pb-2">
        <span className="text-[10px] font-bold text-summit-accent tracking-[0.1em] uppercase">
          Conversations
        </span>
      </div>
      <div className="flex-1 px-2 pb-2 flex flex-col gap-0.5">
        {trustedPeers.length === 0 ? (
          <div className="flex-1 flex items-center justify-center px-3">
            <div className="text-center">
              <div className="text-[10px] text-white/20">No peers</div>
              <div className="text-[9px] text-white/10 mt-1">Trust a peer to message</div>
            </div>
          </div>
        ) : (
          trustedPeers.map(peer => {
            const active = selected === peer.public_key;
            return (
              <button
                key={peer.public_key}
                onClick={() => onSelect(peer.public_key)}
                className={`
                  w-full text-left px-3 py-2.5 rounded-lg flex items-center gap-2.5 transition-all duration-100 cursor-pointer
                  ${active
                    ? 'bg-summit-accent/10 border border-summit-accent/20'
                    : 'border border-transparent hover:bg-white/[0.03]'}
                `}
              >
                <PulseDot
                  color={peer.last_seen_secs < 5 ? '#3fb950' : 'rgba(255,255,255,0.12)'}
                  size={6}
                />
                <div className="flex flex-col min-w-0">
                  <span className={`text-[11px] font-bold truncate ${active ? 'text-summit-accent' : 'text-summit-cream'}`}>
                    {shortKey(peer.public_key)}
                  </span>
                  <span className="text-[8px] text-white/15 truncate">{peer.addr}</span>
                </div>
              </button>
            );
          })
        )}
      </div>
    </div>
  );
}
