import { useMeshGraph } from '../../hooks/useMeshGraph';
import { useUIState } from '../../hooks/useUIState';
import MeshCanvas from './MeshCanvas';

export default function NetworkMap() {
  const { nodes, links } = useMeshGraph();
  const { selectNode } = useUIState();

  const handleNodeClick = (node) => {
    if (node.type !== 'self') {
      selectNode(node.id);
    }
  };

  return (
    <div className="h-full w-full relative bg-summit-bg">
      <MeshCanvas nodes={nodes} links={links} onNodeClick={handleNodeClick} />
    </div>
  );
}
