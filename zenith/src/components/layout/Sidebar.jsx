import { useUIState } from '../../hooks/useUIState';

const NAV_ITEMS = [
  { id: 'home', label: 'Home', icon: HomeIcon },
  { id: 'compute', label: 'Compute', icon: ComputeIcon },
  { id: 'files', label: 'Files', icon: FilesIcon },
  { id: 'messages', label: 'Msgs', icon: MessagesIcon },
  { id: 'system', label: 'System', icon: SystemIcon },
];

export default function Sidebar() {
  const { activeView, setActiveView } = useUIState();

  return (
    <nav className="w-[60px] h-full bg-summit-surface border-r border-summit-border flex flex-col items-center pt-5 pb-3 gap-1.5">
      {/* Logo mark */}
      <div className="mb-4 w-8 h-8 rounded-lg bg-summit-accent/10 border border-summit-accent/20 flex items-center justify-center">
        <svg width="14" height="14" viewBox="0 0 14 14" fill="none">
          <path d="M7 1L13 5v4L7 13 1 9V5z" stroke="#58a6ff" strokeWidth="1.5" fill="none"/>
          <circle cx="7" cy="7" r="2" fill="#58a6ff" opacity="0.6"/>
        </svg>
      </div>

      {NAV_ITEMS.map(item => {
        const active = activeView === item.id;
        const Icon = item.icon;
        return (
          <button
            key={item.id}
            onClick={() => setActiveView(item.id)}
            className={`
              group relative w-10 h-10 flex flex-col items-center justify-center gap-[3px] rounded-lg
              transition-all duration-150 cursor-pointer
              ${active
                ? 'text-summit-accent bg-summit-accent/10'
                : 'text-white/25 hover:text-white/50 hover:bg-white/[0.03]'}
            `}
            title={item.label}
          >
            {active && (
              <span className="absolute left-0 top-1/2 -translate-y-1/2 w-[2px] h-5 bg-summit-accent rounded-r" />
            )}
            <Icon active={active} />
            <span className="text-[8px] tracking-[0.08em] uppercase leading-none">
              {item.label}
            </span>
          </button>
        );
      })}

      <div className="flex-1" />

      {/* Bottom indicator */}
      <div className="w-2 h-2 rounded-full bg-summit-accent/30" title="Connected" />
    </nav>
  );
}

function HomeIcon({ active }) {
  return (
    <svg width="16" height="16" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.3" strokeLinecap="round" strokeLinejoin="round">
      <path d="M3 6.5L8 2.5l5 4v6.5a1 1 0 01-1 1H4a1 1 0 01-1-1z" />
      <path d="M6.5 13V9h3v4" />
    </svg>
  );
}

function ComputeIcon({ active }) {
  return (
    <svg width="16" height="16" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.3" strokeLinecap="round" strokeLinejoin="round">
      <rect x="2" y="3" width="12" height="10" rx="1.5" />
      <path d="M5 7h6M5 9.5h4" />
    </svg>
  );
}

function FilesIcon({ active }) {
  return (
    <svg width="16" height="16" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.3" strokeLinecap="round" strokeLinejoin="round">
      <path d="M9 2H4.5A1.5 1.5 0 003 3.5v9A1.5 1.5 0 004.5 14h7a1.5 1.5 0 001.5-1.5V6z" />
      <path d="M9 2v4h4" />
    </svg>
  );
}

function MessagesIcon({ active }) {
  return (
    <svg width="16" height="16" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.3" strokeLinecap="round" strokeLinejoin="round">
      <path d="M3 3.5h10A1.5 1.5 0 0114.5 5v5a1.5 1.5 0 01-1.5 1.5H5L2.5 14V5A1.5 1.5 0 014 3.5z" />
    </svg>
  );
}

function SystemIcon({ active }) {
  return (
    <svg width="16" height="16" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.3" strokeLinecap="round" strokeLinejoin="round">
      <circle cx="8" cy="8" r="2.5" />
      <path d="M8 1.5v2M8 12.5v2M1.5 8h2M12.5 8h2M3.4 3.4l1.4 1.4M11.2 11.2l1.4 1.4M3.4 12.6l1.4-1.4M11.2 4.8l1.4-1.4" />
    </svg>
  );
}
