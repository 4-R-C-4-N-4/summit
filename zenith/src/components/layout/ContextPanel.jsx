import { useUIState } from '../../hooks/useUIState';
import NodeInspector from '../inspector/NodeInspector';

export default function ContextPanel() {
  const { contextPanelOpen, selectedNode, closePanel } = useUIState();

  if (!contextPanelOpen) return null;

  return (
    <div
      className="w-[380px] h-full border-l border-summit-border bg-summit-surface overflow-y-auto"
      style={{ animation: 'slideIn 0.2s ease-out' }}
    >
      <div className="flex items-center justify-between px-4 py-3 border-b border-summit-border">
        <span className="text-[10px] font-bold text-summit-accent tracking-[0.1em] uppercase">
          Inspector
        </span>
        <button
          onClick={closePanel}
          className="w-6 h-6 flex items-center justify-center rounded text-white/25 hover:text-white/50 hover:bg-white/5 transition-colors cursor-pointer"
        >
          <svg width="12" height="12" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round">
            <path d="M4 4l8 8M12 4l-8 8" />
          </svg>
        </button>
      </div>
      {selectedNode && <NodeInspector nodeId={selectedNode} />}
    </div>
  );
}
