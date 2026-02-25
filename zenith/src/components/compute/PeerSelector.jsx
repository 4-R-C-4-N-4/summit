import { useState, useRef, useEffect } from 'react';
import { useDaemon } from '../../hooks/useDaemon';
import { shortKey } from '../../lib/format';
import PulseDot from '../common/PulseDot';

export default function PeerSelector({ value, onChange }) {
  const { peers } = useDaemon();
  const [open, setOpen] = useState(false);
  const ref = useRef(null);

  const trustedPeers = peers.filter(p => p.trust_level === 'Trusted');
  const selected = trustedPeers.find(p => p.public_key === value);

  useEffect(() => {
    const handler = (e) => {
      if (ref.current && !ref.current.contains(e.target)) setOpen(false);
    };
    document.addEventListener('mousedown', handler);
    return () => document.removeEventListener('mousedown', handler);
  }, []);

  return (
    <div className="flex flex-col gap-1.5">
      <label className="text-[10px] text-white/35 tracking-[0.1em] uppercase font-bold">
        Target Peer
      </label>
      <div ref={ref} className="relative">
        <button
          type="button"
          onClick={() => setOpen(!open)}
          className={`
            w-full flex items-center justify-between
            bg-summit-bg border rounded-lg px-3 py-2.5
            text-[11px] transition-all cursor-pointer
            ${open
              ? 'border-summit-accent/30 ring-1 ring-summit-accent/10'
              : 'border-white/10 hover:border-white/15'}
          `}
        >
          {selected ? (
            <span className="flex items-center gap-2">
              <PulseDot color={selected.last_seen_secs < 5 ? '#3fb950' : 'rgba(255,255,255,0.15)'} size={5} />
              <span className="text-summit-cream">{shortKey(selected.public_key)}</span>
              <span className="text-white/20">{selected.addr}</span>
            </span>
          ) : (
            <span className="text-white/25">Select a peer...</span>
          )}
          <svg width="12" height="12" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="2" className={`text-white/20 transition-transform ${open ? 'rotate-180' : ''}`}>
            <path d="M4 6l4 4 4-4" />
          </svg>
        </button>

        {open && (
          <div className="absolute z-20 top-full mt-1 left-0 right-0 bg-summit-raised border border-summit-border-bright rounded-lg shadow-xl shadow-black/40 overflow-hidden">
            {trustedPeers.length === 0 ? (
              <div className="px-3 py-4 text-center text-[10px] text-white/25">
                No trusted peers available
              </div>
            ) : (
              trustedPeers.map(peer => (
                <button
                  key={peer.public_key}
                  type="button"
                  onClick={() => { onChange(peer.public_key); setOpen(false); }}
                  className={`
                    w-full flex items-center gap-2 px-3 py-2.5 text-left transition-colors cursor-pointer
                    ${peer.public_key === value
                      ? 'bg-summit-accent/10 text-summit-accent'
                      : 'text-summit-cream hover:bg-white/[0.03]'}
                  `}
                >
                  <PulseDot color={peer.last_seen_secs < 5 ? '#3fb950' : 'rgba(255,255,255,0.15)'} size={5} />
                  <span className="text-[11px] font-bold">{shortKey(peer.public_key)}</span>
                  <span className="text-[10px] text-white/20 ml-auto">{peer.addr}</span>
                </button>
              ))
            )}
          </div>
        )}
      </div>
    </div>
  );
}
