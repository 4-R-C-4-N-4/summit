import { useState } from 'react';

export default function MessageInput({ onSend, disabled }) {
  const [text, setText] = useState('');

  const handleSend = () => {
    if (!text.trim() || disabled) return;
    onSend(text.trim());
    setText('');
  };

  const canSend = text.trim() && !disabled;

  return (
    <div className="px-4 py-3 border-t border-summit-border bg-summit-surface/50 flex gap-2">
      <input
        type="text"
        value={text}
        onChange={(e) => setText(e.target.value)}
        onKeyDown={(e) => e.key === 'Enter' && handleSend()}
        placeholder="Type a message..."
        disabled={disabled}
        className="flex-1 px-3.5 py-2 bg-summit-bg border border-white/8 rounded-lg text-[11px] text-summit-cream outline-none focus:border-summit-accent/25 focus:ring-1 focus:ring-summit-accent/10 transition-all placeholder:text-white/15"
      />
      <button
        onClick={handleSend}
        disabled={!canSend}
        className={`
          px-4 py-2 rounded-lg text-[10px] font-bold tracking-[0.06em] uppercase transition-all duration-150
          ${canSend
            ? 'bg-summit-accent/15 border border-summit-accent/25 text-summit-accent hover:bg-summit-accent/20 cursor-pointer active:scale-[0.97]'
            : 'bg-white/[0.03] border border-white/8 text-white/15 cursor-not-allowed'}
        `}
      >
        Send
      </button>
    </div>
  );
}
