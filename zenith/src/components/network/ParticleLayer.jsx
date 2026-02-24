import { useRef, useEffect } from 'react';
import { COLORS } from '../../lib/colors';

export default function ParticleLayer({ links, nodesRef, width, height }) {
  const canvasRef = useRef(null);
  const particlesRef = useRef([]);

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const ctx = canvas.getContext('2d');
    let animId;

    const animate = () => {
      ctx.clearRect(0, 0, width, height);

      // Spawn particles on active links
      links.forEach(link => {
        if (!link.active) return;
        if (Math.random() < 0.02) {
          const source = typeof link.source === 'object' ? link.source : nodesRef.current?.find(n => n.id === link.source);
          const target = typeof link.target === 'object' ? link.target : nodesRef.current?.find(n => n.id === link.target);
          if (source && target) {
            particlesRef.current.push({
              sx: source.x, sy: source.y,
              tx: target.x, ty: target.y,
              t: 0,
            });
          }
        }
      });

      // Update and draw
      particlesRef.current = particlesRef.current.filter(p => {
        p.t += 0.015;
        if (p.t > 1) return false;
        const x = p.sx + (p.tx - p.sx) * p.t;
        const y = p.sy + (p.ty - p.sy) * p.t;
        ctx.beginPath();
        ctx.arc(x, y, 2, 0, Math.PI * 2);
        ctx.fillStyle = `${COLORS.amber}${Math.round((1 - p.t) * 200).toString(16).padStart(2, '0')}`;
        ctx.fill();
        return true;
      });

      animId = requestAnimationFrame(animate);
    };

    animate();
    return () => cancelAnimationFrame(animId);
  }, [links, nodesRef, width, height]);

  return (
    <canvas
      ref={canvasRef}
      width={width}
      height={height}
      className="absolute inset-0 pointer-events-none"
    />
  );
}
