import { useRef, useEffect } from 'react';
import { useCanvasSize } from '../../hooks/useCanvasSize';
import { useForceSimulation } from '../../hooks/useForceSimulation';
import { COLORS } from '../../lib/colors';

export default function MeshCanvas({ nodes, links, onNodeClick }) {
  const containerRef = useRef(null);
  const canvasRef = useRef(null);
  const { width, height } = useCanvasSize(containerRef);
  const { nodesRef, linksRef, onTick } = useForceSimulation(nodes, links, { width, height });

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const ctx = canvas.getContext('2d');
    const dpr = window.devicePixelRatio || 1;
    canvas.width = width * dpr;
    canvas.height = height * dpr;
    ctx.scale(dpr, dpr);

    const draw = () => {
      ctx.clearRect(0, 0, width, height);

      // Draw links
      linksRef.current.forEach(link => {
        const source = typeof link.source === 'object' ? link.source : nodesRef.current.find(n => n.id === link.source);
        const target = typeof link.target === 'object' ? link.target : nodesRef.current.find(n => n.id === link.target);
        if (!source || !target) return;

        // Session glow
        if (link.session) {
          ctx.beginPath();
          ctx.moveTo(source.x, source.y);
          ctx.lineTo(target.x, target.y);
          ctx.strokeStyle = `${COLORS.accent}18`;
          ctx.lineWidth = 8;
          ctx.stroke();
        }

        ctx.beginPath();
        ctx.moveTo(source.x, source.y);
        ctx.lineTo(target.x, target.y);
        ctx.strokeStyle = link.active
          ? `${COLORS.accent}55`
          : `${COLORS.cream}12`;
        ctx.lineWidth = link.session ? 1.5 : 0.8;
        ctx.stroke();
      });

      // Draw nodes
      nodesRef.current.forEach(node => {
        const isSelf = node.type === 'self';
        const radius = isSelf ? 8 : 5;
        const color = isSelf
          ? COLORS.accent
          : node.trust === 'Trusted'
            ? COLORS.green
            : node.trust === 'Blocked'
              ? COLORS.red
              : COLORS.cream;

        // Outer glow
        const gradient = ctx.createRadialGradient(node.x, node.y, radius, node.x, node.y, radius + 12);
        gradient.addColorStop(0, `${color}15`);
        gradient.addColorStop(1, `${color}00`);
        ctx.beginPath();
        ctx.arc(node.x, node.y, radius + 12, 0, Math.PI * 2);
        ctx.fillStyle = gradient;
        ctx.fill();

        // Node circle
        ctx.beginPath();
        ctx.arc(node.x, node.y, radius, 0, Math.PI * 2);
        ctx.fillStyle = isSelf ? `${color}dd` : `${color}88`;
        ctx.fill();

        // Inner highlight
        if (isSelf) {
          ctx.beginPath();
          ctx.arc(node.x, node.y, radius - 2, 0, Math.PI * 2);
          ctx.fillStyle = `${color}40`;
          ctx.fill();
        }

        // Label
        ctx.font = '9px "Space Mono", monospace';
        ctx.fillStyle = `${COLORS.cream}66`;
        ctx.textAlign = 'center';
        ctx.fillText(node.label, node.x, node.y + radius + 14);
      });
    };

    onTick(draw);
    draw();
  }, [width, height, onTick, nodesRef, linksRef]);

  const handleClick = (e) => {
    if (!onNodeClick) return;
    const rect = canvasRef.current.getBoundingClientRect();
    const x = e.clientX - rect.left;
    const y = e.clientY - rect.top;

    for (const node of nodesRef.current) {
      const dx = node.x - x;
      const dy = node.y - y;
      if (dx * dx + dy * dy < 20 * 20) {
        onNodeClick(node);
        return;
      }
    }
  };

  return (
    <div ref={containerRef} className="w-full h-full">
      <canvas
        ref={canvasRef}
        style={{ width, height }}
        onClick={handleClick}
        className="cursor-crosshair"
      />
    </div>
  );
}
