import { useState, useEffect, useRef } from 'react';
import { api } from '../../api/client';
import { useDaemon } from '../../hooks/useDaemon';
import { shortKey } from '../../lib/format';
import MessageBubble from './MessageBubble';
import MessageInput from './MessageInput';

export default function ChatWindow({ peerKey }) {
  const { sendMessage } = useDaemon();
  const [messages, setMessages] = useState([]);
  const scrollRef = useRef(null);

  useEffect(() => {
    if (!peerKey) return;
    const fetchMessages = async () => {
      try {
        const data = await api.getMessages(peerKey);
        setMessages(data.messages || []);
      } catch {}
    };
    fetchMessages();
    const id = setInterval(fetchMessages, 2000);
    return () => clearInterval(id);
  }, [peerKey]);

  useEffect(() => {
    scrollRef.current?.scrollTo({ top: scrollRef.current.scrollHeight, behavior: 'smooth' });
  }, [messages]);

  const handleSend = async (text) => {
    await sendMessage(peerKey, text);
    setMessages(prev => [...prev, { from: 'self', content: { text }, timestamp: Date.now() }]);
  };

  if (!peerKey) {
    return (
      <div className="flex-1 flex items-center justify-center">
        <div className="text-center">
          <svg width="32" height="32" viewBox="0 0 32 32" fill="none" stroke="rgba(255,255,255,0.08)" strokeWidth="1.5" className="mx-auto mb-3">
            <path d="M6 7h20A3 3 0 0129 10v10a3 3 0 01-3 3H10L5 28V10a3 3 0 013-3z" />
          </svg>
          <div className="text-[11px] text-white/20">Select a conversation</div>
          <div className="text-[9px] text-white/10 mt-1">Choose a peer from the sidebar</div>
        </div>
      </div>
    );
  }

  return (
    <div className="flex-1 flex flex-col min-h-0">
      <div className="px-4 py-2.5 border-b border-summit-border bg-summit-surface/50 flex items-center gap-2">
        <span className="text-[12px] font-bold text-summit-white">
          {shortKey(peerKey)}
        </span>
        <span className="text-[9px] text-white/15 truncate">{peerKey}</span>
      </div>
      <div ref={scrollRef} className="flex-1 overflow-y-auto p-4 flex flex-col gap-2">
        {messages.length === 0 ? (
          <div className="flex-1 flex items-center justify-center">
            <div className="text-[10px] text-white/15">No messages yet — say hello</div>
          </div>
        ) : (
          messages.map((msg, i) => (
            <MessageBubble key={i} message={msg} isSent={msg.from !== peerKey} />
          ))
        )}
      </div>
      <MessageInput onSend={handleSend} />
    </div>
  );
}
