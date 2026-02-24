import { useState } from 'react';
import { useDaemon } from '../../hooks/useDaemon';

export default function TrustControls({ publicKey, trustLevel }) {
  const { trustPeer, blockPeer } = useDaemon();
  const [loading, setLoading] = useState(null);

  const isTrusted = trustLevel === 'Trusted';
  const isBlocked = trustLevel === 'Blocked';

  if (isTrusted || isBlocked) return null;

  const handleAction = async (action) => {
    setLoading(action);
    try {
      if (action === 'trust') await trustPeer(publicKey);
      else await blockPeer(publicKey);
    } finally {
      setLoading(null);
    }
  };

  return (
    <div className="flex gap-2">
      <button
        onClick={() => handleAction('trust')}
        disabled={!!loading}
        className="flex-1 bg-summit-green/10 border border-summit-green/20 rounded-lg px-3 py-2.5 text-[10px] font-bold text-summit-green cursor-pointer hover:bg-summit-green/15 active:scale-[0.98] transition-all disabled:opacity-50"
      >
        {loading === 'trust' ? 'Trusting...' : 'Trust'}
      </button>
      <button
        onClick={() => handleAction('block')}
        disabled={!!loading}
        className="flex-1 bg-summit-red/10 border border-summit-red/20 rounded-lg px-3 py-2.5 text-[10px] font-bold text-summit-red cursor-pointer hover:bg-summit-red/15 active:scale-[0.98] transition-all disabled:opacity-50"
      >
        {loading === 'block' ? 'Blocking...' : 'Block'}
      </button>
    </div>
  );
}
