import { useDaemon } from '../../hooks/useDaemon';
import { shortKey, timeAgo } from '../../lib/format';
import Badge from '../common/Badge';
import CopyButton from '../common/CopyButton';
import TrustControls from './TrustControls';
import PulseDot from '../common/PulseDot';

export default function NodeInspector({ nodeId }) {
  const { peers, trust } = useDaemon();
  const peer = peers.find(p => p.public_key === nodeId);
  const trustMap = {};
  trust.forEach(t => { trustMap[t.public_key] = t.level; });
  const trustLevel = trustMap[nodeId] || 'Untrusted';

  if (!peer) {
    return (
      <div className="p-6 text-center">
        <div className="text-[11px] text-white/20">Peer not found</div>
        <div className="text-[9px] text-white/10 mt-1">This peer may have disconnected</div>
      </div>
    );
  }

  const isActive = peer.last_seen_secs < 5;

  return (
    <div className="p-4 flex flex-col gap-5">
      {/* Identity */}
      <div>
        <div className="flex items-center gap-2.5 mb-2">
          <PulseDot color={isActive ? '#3fb950' : 'rgba(255,255,255,0.12)'} size={7} />
          <span className="text-[14px] font-bold text-summit-white">
            {shortKey(peer.public_key)}
          </span>
          <Badge
            variant={trustLevel === 'Trusted' ? 'trusted' : trustLevel === 'Blocked' ? 'blocked' : 'pending'}
            label={trustLevel.toLowerCase()}
          />
        </div>
        <div className="flex items-center gap-2 pl-[19px]">
          <span className="text-[9px] text-white/20 truncate">{peer.public_key}</span>
          <CopyButton text={peer.public_key} />
        </div>
      </div>

      {/* Details grid */}
      <div className="bg-summit-raised/50 border border-summit-border rounded-lg p-3 grid grid-cols-2 gap-3">
        <InfoRow label="Address" value={peer.addr} />
        <InfoRow label="Status" value={isActive ? 'Online' : timeAgo(peer.last_seen_secs)} highlight={isActive} />
        {peer.buffered_chunks > 0 && (
          <InfoRow label="Buffered" value={`${peer.buffered_chunks} chunks`} />
        )}
      </div>

      <TrustControls publicKey={peer.public_key} trustLevel={trustLevel} />
    </div>
  );
}

function InfoRow({ label, value, highlight }) {
  return (
    <div>
      <div className="text-[9px] text-white/25 tracking-wider uppercase mb-0.5">{label}</div>
      <div className={`text-[11px] ${highlight ? 'text-summit-green' : 'text-summit-cream'}`}>{value}</div>
    </div>
  );
}
