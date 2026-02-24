import { useState } from 'react';
import { useDaemon } from '../../hooks/useDaemon';
import { bytesFormat } from '../../lib/format';
import SectionHeader from '../common/SectionHeader';

export default function CachePanel() {
  const { cache, clearCache } = useDaemon();
  const [clearing, setClearing] = useState(false);

  const handleClear = async () => {
    setClearing(true);
    try {
      await clearCache();
    } finally {
      setClearing(false);
    }
  };

  return (
    <div>
      <SectionHeader label="Chunk Cache" accent="text-summit-blue" />
      <div className="bg-summit-raised/40 border border-summit-border rounded-xl p-4 flex items-center justify-between">
        <div className="flex gap-8">
          <div>
            <div className="text-[10px] text-white/25 mb-0.5">Chunks</div>
            <div className="text-lg font-bold text-summit-cream tabular-nums">{cache?.chunks || 0}</div>
          </div>
          <div>
            <div className="text-[10px] text-white/25 mb-0.5">Size</div>
            <div className="text-lg font-bold text-summit-cream tabular-nums">{bytesFormat(cache?.bytes || 0)}</div>
          </div>
        </div>
        <button
          onClick={handleClear}
          disabled={clearing || (cache?.chunks || 0) === 0}
          className={`
            px-3 py-1.5 rounded-md text-[10px] font-bold tracking-wide transition-all duration-150 cursor-pointer
            ${(cache?.chunks || 0) > 0 && !clearing
              ? 'bg-summit-red/10 border border-summit-red/20 text-summit-red hover:bg-summit-red/15'
              : 'bg-white/[0.03] border border-white/8 text-white/15 cursor-not-allowed'}
          `}
        >
          {clearing ? 'Clearing...' : 'Clear'}
        </button>
      </div>
    </div>
  );
}
