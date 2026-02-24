import { useState } from 'react';
import ConversationList from './ConversationList';
import ChatWindow from './ChatWindow';

export default function MessagesView() {
  const [selectedPeer, setSelectedPeer] = useState(null);

  return (
    <div className="h-full flex" style={{ animation: 'fadeIn 0.25s ease-out' }}>
      <ConversationList selected={selectedPeer} onSelect={setSelectedPeer} />
      <ChatWindow peerKey={selectedPeer} />
    </div>
  );
}
