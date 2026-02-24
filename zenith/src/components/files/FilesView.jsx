import { useDaemon } from '../../hooks/useDaemon';
import SectionHeader from '../common/SectionHeader';
import DropZone from './DropZone';
import FileList from './FileList';
import TransferCard from './TransferCard';

export default function FilesView() {
  const { files, sendFile } = useDaemon();

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
