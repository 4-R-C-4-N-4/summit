export default function MessageBubble({ message, isSent }) {
  return (
    <div className={`max-w-[70%] ${isSent ? 'self-end' : 'self-start'}`}>
      <div
        className={`
          px-3.5 py-2 rounded-xl border
          ${isSent
            ? 'bg-summit-accent/12 border-summit-accent/20 rounded-br-sm'
            : 'bg-white/[0.04] border-white/8 rounded-bl-sm'}
        `}
      >
        <div className="text-[11px] text-summit-cream break-words leading-relaxed">
          {message.content?.text ?? message.text}
        </div>
        <div className="text-[8px] text-white/20 mt-1.5 tabular-nums">
          {message.timestamp
            ? new Date(message.timestamp).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })
            : ''}
        </div>
      </div>
    </div>
  );
}
