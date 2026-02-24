import SectionHeader from '../common/SectionHeader';

export default function FileList({ files }) {
  if (!files || files.length === 0) return null;

  return (
    <div>
      <SectionHeader label="Received Files" count={files.length} accent="text-summit-green" />
      <div className="flex flex-col gap-1.5">
        {files.map((filename, i) => (
          <div
            key={i}
            className="bg-summit-green/5 border border-summit-green/15 rounded-lg px-3 py-2.5 text-[11px] text-summit-green flex items-center gap-2"
          >
            <svg width="12" height="12" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round">
              <path d="M3.5 8.5l3 3 6-7" />
            </svg>
            {filename}
          </div>
        ))}
      </div>
    </div>
  );
}
