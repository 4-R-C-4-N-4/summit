import { useDaemon } from '../../hooks/useDaemon';
import { useConfig } from '../../hooks/useConfig';
import SectionHeader from '../common/SectionHeader';
import DropZone from './DropZone';
import FileList from './FileList';
import TransferCard from './TransferCard';

export default function FilesView() {
  const { files, sendFile } = useDaemon();
  const { config, openStoragePath } = useConfig();

  const storagePath = config?.services?.file_transfer_settings?.storage_path;

  const handleFiles = (fileList) => {
    fileList.forEach(file => sendFile(file, { type: 'broadcast' }));
  };

  return (
    <div className="p-6 overflow-auto h-full" style={{ animation: 'fadeIn 0.25s ease-out' }}>
      <div className="max-w-3xl mx-auto flex flex-col gap-5">
        <div>
          <h2 className="text-xl font-bold text-summit-white tracking-[0.08em] uppercase mb-1">
            Files
          </h2>
          <p className="text-[10px] text-white/25 tracking-wide">
            Transfer files across the mesh
          </p>
        </div>

        {/* Storage path banner */}
        {storagePath && (
          <div className="flex items-center justify-between bg-summit-raised/60 border border-summit-border rounded-xl px-4 py-3">
            <div className="flex items-center gap-3 min-w-0">
              <svg width="14" height="14" viewBox="0 0 16 16" fill="none" stroke="rgba(88,166,255,0.5)" strokeWidth="1.4" strokeLinecap="round" strokeLinejoin="round" className="shrink-0">
                <path d="M1.5 4.5A1.5 1.5 0 013 3h3l2 2h5a1.5 1.5 0 011.5 1.5v6A1.5 1.5 0 0113 14H3a1.5 1.5 0 01-1.5-1.5z" />
              </svg>
              <div className="min-w-0">
                <div className="text-[9px] text-white/25 tracking-wider uppercase mb-0.5">Storage directory</div>
                <div className="text-[11px] text-summit-cream truncate font-mono">{storagePath}</div>
              </div>
            </div>
            <button
              onClick={() => openStoragePath(storagePath)}
              className="shrink-0 ml-3 flex items-center gap-1.5 px-2.5 py-1.5 rounded-lg text-[9px] text-white/30 hover:text-summit-accent hover:bg-summit-accent/8 border border-transparent hover:border-summit-accent/15 transition-all cursor-pointer"
              title="Open in file manager"
            >
              <svg width="11" height="11" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round">
                <path d="M7 3H3a1 1 0 00-1 1v9a1 1 0 001 1h10a1 1 0 001-1V7" />
                <path d="M10 2h4v4" />
                <path d="M8 8l6-6" />
              </svg>
              Open
            </button>
          </div>
        )}

        <DropZone onFiles={handleFiles} />

        {files?.in_progress?.length > 0 && (
          <div>
            <SectionHeader label="In Progress" count={files.in_progress.length} />
            <div className="flex flex-col gap-1.5">
              {files.in_progress.map((f, i) => (
                <TransferCard key={i} filename={f} />
              ))}
            </div>
          </div>
        )}

        <FileList files={files?.received} />
      </div>
    </div>
  );
}
