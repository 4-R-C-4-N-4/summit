import { useRef, useEffect, useCallback } from 'react';
import { forceSimulation, forceLink, forceManyBody, forceCenter, forceCollide } from 'd3-force';

export function useForceSimulation(nodes, links, { width, height }) {
  const simRef = useRef(null);
  const nodesRef = useRef([]);
  const linksRef = useRef([]);
  const tickCallbackRef = useRef(null);

  useEffect(() => {
    // Merge positions from existing nodes
    const existing = new Map(nodesRef.current.map(n => [n.id, n]));
    const newNodes = nodes.map(n => {
      const prev = existing.get(n.id);
      return prev
        ? { ...n, x: prev.x, y: prev.y, vx: prev.vx, vy: prev.vy }
        : { ...n, x: width / 2 + (Math.random() - 0.5) * 100, y: height / 2 + (Math.random() - 0.5) * 100 };
    });

    const newLinks = links.map(l => ({ ...l }));

    nodesRef.current = newNodes;
    linksRef.current = newLinks;

    if (simRef.current) simRef.current.stop();

    simRef.current = forceSimulation(newNodes)
      .force('link', forceLink(newLinks).id(d => d.id).distance(100))
      .force('charge', forceManyBody().strength(-200))
      .force('center', forceCenter(width / 2, height / 2))
      .force('collide', forceCollide(30))
      .alphaDecay(0.02)
      .on('tick', () => {
        tickCallbackRef.current?.();
      });

    return () => {
      if (simRef.current) simRef.current.stop();
    };
  }, [nodes, links, width, height]);

  const onTick = useCallback((cb) => {
    tickCallbackRef.current = cb;
  }, []);

  return { nodesRef, linksRef, onTick };
}
