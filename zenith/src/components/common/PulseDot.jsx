export default function PulseDot({ color = '#58a6ff', size = 8 }) {
  return (
    <span className="relative inline-block" style={{ width: size, height: size }}>
      <span
        className="absolute inset-0 rounded-full"
        style={{ background: color, opacity: 0.3, animation: 'pulseRing 2s ease-out infinite' }}
      />
      <span
        className="absolute rounded-full"
        style={{ inset: '2px', background: color }}
      />
    </span>
  );
}
