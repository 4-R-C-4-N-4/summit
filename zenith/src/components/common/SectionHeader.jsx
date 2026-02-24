export default function SectionHeader({ label, count, accent = 'text-summit-accent' }) {
  return (
    <div className="flex items-center gap-2.5 mb-3">
      <span className={`text-[10px] font-bold ${accent} tracking-[0.1em] uppercase`}>
        {label}
      </span>
      {count != null && (
        <span className="text-[10px] text-white/30 bg-white/[0.04] px-2 py-0.5 rounded-md font-bold tabular-nums">
          {count}
        </span>
      )}
      <div className="flex-1 h-px bg-gradient-to-r from-summit-border-bright to-transparent" />
    </div>
  );
}
