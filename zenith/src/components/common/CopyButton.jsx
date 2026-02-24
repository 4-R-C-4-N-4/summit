import { useState } from 'react';

export default function CopyButton({ text, className = '' }) {
  const [copied, setCopied] = useState(false);

  const handleCopy = async () => {
    await navigator.clipboard.writeText(text);
    setCopied(true);
    setTimeout(() => setCopied(false), 1500);
  };

  return (
    <button
      onClick={handleCopy}
      className={`
        inline-flex items-center gap-1 px-1.5 py-0.5 rounded
        text-[9px] tracking-wider transition-all duration-150 cursor-pointer
        ${copied
          ? 'text-summit-green bg-summit-green/10'
          : 'text-white/30 hover:text-summit-accent hover:bg-summit-accent/5'}
        ${className}
      `}
    >
      <svg width="10" height="10" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round">
        <rect x="5" y="5" width="9" height="9" rx="1.5" />
        <path d="M3 11V3.5A1.5 1.5 0 014.5 2H11" />
      </svg>
      {copied ? 'COPIED' : 'COPY'}
    </button>
  );
}
