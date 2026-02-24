import { useState, useEffect, useRef } from 'react';

export function useCanvasSize(containerRef) {
  const [size, setSize] = useState({ width: 400, height: 300 });

  useEffect(() => {
    if (!containerRef.current) return;
    const observer = new ResizeObserver(([entry]) => {
      const { width, height } = entry.contentRect;
      setSize({ width: Math.floor(width), height: Math.floor(height) });
    });
    observer.observe(containerRef.current);
    return () => observer.disconnect();
  }, [containerRef]);

  return size;
}
