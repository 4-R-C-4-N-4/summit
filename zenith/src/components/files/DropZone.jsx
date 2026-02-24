import { useRef, useState } from 'react';

export default function DropZone({ onFiles }) {
  const inputRef = useRef(null);
  const [dragging, setDragging] = useState(false);

  const handleDrop = (e) => {
    e.preventDefault();
    setDragging(false);
    const files = Array.from(e.dataTransfer.files);
    if (files.length) onFiles(files);
  };

  const handleSelect = (e) => {
    const files = Array.from(e.target.files);
    if (files.length) onFiles(files);
  };

  return (
    <div
      onDragOver={(e) => { e.preventDefault(); setDragging(true); }}
      onDragLeave={() => setDragging(false)}
      onDrop={handleDrop}
      onClick={() => inputRef.current?.click()}
      className={`
        border-2 border-dashed rounded-xl p-10 flex flex-col items-center justify-center gap-3
        cursor-pointer transition-all duration-200
        ${dragging
          ? 'border-summit-accent/50 bg-summit-accent/8 scale-[1.01]'
          : 'border-white/8 hover:border-summit-accent/25 bg-summit-raised/30'}
      `}
    >
      <input ref={inputRef} type="file" multiple onChange={handleSelect} className="hidden" />
      <div className={`
        w-10 h-10 rounded-lg flex items-center justify-center transition-colors
        ${dragging ? 'bg-summit-accent/15' : 'bg-white/[0.04]'}
      `}>
        <svg width="20" height="20" viewBox="0 0 20 20" fill="none" stroke={dragging ? '#58a6ff' : 'rgba(255,255,255,0.25)'} strokeWidth="1.5" strokeLinecap="round">
          <path d="M10 4v12M4 10h12" />
        </svg>
      </div>
      <div className="text-center">
        <div className={`text-[12px] ${dragging ? 'text-summit-accent' : 'text-white/35'} transition-colors`}>
          Drop files or click to browse
        </div>
        <div className="text-[9px] text-white/15 mt-1">
          Files will be broadcast to the mesh
        </div>
      </div>
    </div>
  );
}
