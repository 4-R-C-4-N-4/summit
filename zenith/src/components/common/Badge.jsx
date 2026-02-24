const VARIANTS = {
  trusted:   { text: 'text-summit-green',  bg: 'bg-summit-green/15',  ring: 'ring-summit-green/20' },
  blocked:   { text: 'text-summit-red',    bg: 'bg-summit-red/15',    ring: 'ring-summit-red/20' },
  pending:   { text: 'text-summit-accent', bg: 'bg-summit-accent/10', ring: 'ring-summit-accent/15' },
  running:   { text: 'text-summit-blue',   bg: 'bg-summit-blue/15',   ring: 'ring-summit-blue/20' },
  completed: { text: 'text-summit-green',  bg: 'bg-summit-green/15',  ring: 'ring-summit-green/20' },
  failed:    { text: 'text-summit-red',    bg: 'bg-summit-red/15',    ring: 'ring-summit-red/20' },
  queued:    { text: 'text-summit-purple',  bg: 'bg-summit-purple/12', ring: 'ring-summit-purple/15' },
};

export default function Badge({ variant = 'pending', label }) {
  const v = VARIANTS[variant] || VARIANTS.pending;
  return (
    <span className={`
      ${v.text} ${v.bg} ring-1 ${v.ring}
      inline-flex items-center px-2 py-0.5 rounded-md
      text-[9px] font-bold tracking-[0.06em] uppercase leading-none
    `}>
      {label || variant}
    </span>
  );
}
