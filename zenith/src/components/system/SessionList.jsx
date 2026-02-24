import { useDaemon } from '../../hooks/useDaemon';
import { shortKey } from '../../lib/format';
import SectionHeader from '../common/SectionHeader';
import PulseDot from '../common/PulseDot';

export default function SessionList() {
  const { status, dropSession } = useDaemon();
  const sessions = status?.sessions || [];

  return (
    <div>
      <SectionHeader label="Active Sessions" count={sessions.length} />
      {sessions.length === 0 ? (
        <div className="bg-summit-raised/30 border border-summit-border rounded-xl py-8 text-center">
          <div className="text-[11px] text-white/20">No active sessions</div>
          <div className="text-[9px] text-white/10 mt-1">Sessions appear when peers connect</div>
        </div>
      ) : (
        <div className="flex flex-col gap-2">
          {sessions.map(session => (
            <div
              key={session.session_id}
              className="bg-summit-raised/40 border border-summit-border rounded-xl p-4 flex flex-col gap-2.5"
            >
              <div className="flex items-center justify-between">
                <div className="flex items-center gap-2.5">
                  <PulseDot color="#3fb950" size={6} />
                  <span className="text-[12px] font-bold text-summit-cream">
                    {shortKey(session.peer_pubkey)}
                  </span>
                  <span className="text-[9px] text-summit-accent/50 bg-summit-accent/8 px-2 py-0.5 rounded-md">
                    {session.contract}
                  </span>
                </div>
                <button
                  onClick={() => dropSession(session.session_id)}
                  className="bg-summit-red/10 border border-summit-red/20 rounded-md px-2.5 py-1 text-[9px] font-bold text-summit-red cursor-pointer hover:bg-summit-red/15 transition-colors"
                >
                  Drop
                </button>
              </div>
              <div className="flex items-center gap-4 text-[10px] text-white/25">
                <span>session: {shortKey(session.session_id)}</span>
                <span>uptime: {session.established_secs}s</span>
                <span className="text-white/15">{session.peer}</span>
              </div>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
