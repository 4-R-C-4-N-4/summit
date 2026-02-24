import { useMeshGraph } from '../../hooks/useMeshGraph';
import { useUIState } from '../../hooks/useUIState';
import MeshCanvas from '../network/MeshCanvas';

export default function NetworkWidget() {
  const { nodes, links } = useMeshGraph();
  const { selectNode } = useUIState();

  return (
    <div className="bg-summit-raised/60 border border-summit-border rounded-xl overflow-hidden">
      <div className="px-4 py-2.5 border-b border-summit-border flex items-center justify-between">
        <span className="text-[10px] font-bold text-summit-accent tracking-[0.1em] uppercase">
          Network Topology
        </span>
        <span className="text-[10px] text-white/20 tabular-nums">
          {nodes.length} node{nodes.length !== 1 ? 's' : ''}
        </span>
      </div>
      <div className="h-80 relative">
        <MeshCanvas
          nodes={nodes}
          links={links}
          onNodeClick={(node) => node.type !== 'self' && selectNode(node.id)}
        />
        {nodes.length <= 1 && (
          <div className="absolute inset-0 flex items-center justify-center pointer-events-none">
            <div className="text-center">
              <div className="text-white/15 text-[11px] tracking-wide">No peers discovered</div>
              <div className="text-white/10 text-[9px] mt-1">Nodes will appear when peers connect</div>
            </div>
          </div>
        )}
      </div>
    </div>
  );
}
