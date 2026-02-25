import { useMemo } from 'react';
import { useDaemon } from './useDaemon';

export function useMeshGraph() {
  const { status, peers } = useDaemon();

  return useMemo(() => {
    const selfKey = status?.public_key || 'self';

    const nodes = [
      { id: selfKey, label: 'You', type: 'self' },
    ];

    const links = [];

    (peers || []).forEach(peer => {
      nodes.push({
        id: peer.public_key,
        label: peer.public_key.slice(0, 8),
        type: 'peer',
        trust: peer.trust_level || 'Untrusted',
        active: peer.last_seen_secs < 5,
        addr: peer.addr,
      });
      links.push({
        source: selfKey,
        target: peer.public_key,
        active: peer.last_seen_secs < 5,
      });
    });

    // Add session edges between peers
    (status?.sessions || []).forEach(session => {
      if (session.peer_pubkey && session.peer_pubkey !== selfKey) {
        const existing = links.find(l =>
          (l.source === selfKey && l.target === session.peer_pubkey) ||
          (l.source === session.peer_pubkey && l.target === selfKey)
        );
        if (existing) existing.session = true;
      }
    });

    return { nodes, links };
  }, [status, peers]);
}
