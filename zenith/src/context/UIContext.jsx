import { createContext, useState, useCallback } from 'react';

export const UIContext = createContext(null);

export function UIProvider({ children }) {
  const [activeView, setActiveView] = useState('home');
  const [selectedNode, setSelectedNode] = useState(null);
  const [contextPanelOpen, setContextPanelOpen] = useState(false);
  const [notifications, setNotifications] = useState([]);

  const selectNode = useCallback((nodeId) => {
    setSelectedNode(nodeId);
    setContextPanelOpen(!!nodeId);
  }, []);

  const closePanel = useCallback(() => {
    setContextPanelOpen(false);
    setSelectedNode(null);
  }, []);

  const notify = useCallback((message, type = 'info') => {
    const id = Date.now();
    setNotifications(prev => [...prev, { id, message, type }]);
    setTimeout(() => {
      setNotifications(prev => prev.filter(n => n.id !== id));
    }, 3000);
  }, []);

  const value = {
    activeView,
    setActiveView,
    selectedNode,
    selectNode,
    contextPanelOpen,
    setContextPanelOpen,
    closePanel,
    notifications,
    notify,
  };

  return (
    <UIContext.Provider value={value}>
      {children}
    </UIContext.Provider>
  );
}
