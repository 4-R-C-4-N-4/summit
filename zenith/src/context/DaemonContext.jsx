import { createContext, useState, useEffect, useCallback } from 'react';
import { api } from '../api/client';

export const DaemonContext = createContext(null);

const EMPTY = {
  status: { sessions: [], peers_discovered: 0 },
  peers: [],
  trust: [],
  files: { received: [], in_progress: [] },
  cache: { chunks: 0, bytes: 0 },
  schemas: [],
  services: [],
  computeTasks: [],
};

export function DaemonProvider({ children }) {
  const [state, setState] = useState(EMPTY);
  const [connected, setConnected] = useState(false);

  const poll = useCallback(async () => {
    try {
      const [status, peers, trust, files, cache, schemas, services, computeTasks] = await Promise.all([
        api.getStatus().catch(() => EMPTY.status),
        api.getPeers().catch(() => ({ peers: [] })),
        api.getTrust().catch(() => ({ rules: [] })),
        api.getFiles().catch(() => EMPTY.files),
        api.getCacheStats().catch(() => EMPTY.cache),
        api.getSchemas().catch(() => ({ schemas: [] })),
        api.getServices().catch(() => ({ services: [] })),
        api.getComputeTasks().catch(() => ({ tasks: [] })),
      ]);

      setState({
        status,
        peers: peers.peers || [],
        trust: trust.rules || [],
        files,
        cache,
        schemas: schemas.schemas || [],
        services: services.services || [],
        computeTasks: computeTasks.tasks || [],
      });
      setConnected(true);
    } catch {
      setConnected(false);
    }
  }, []);

  useEffect(() => {
    poll();
    const id = setInterval(poll, 2000);
    return () => clearInterval(id);
  }, [poll]);

  // Action methods
  const trustPeer = useCallback(async (publicKey) => {
    await api.trustPeer(publicKey);
    poll();
  }, [poll]);

  const blockPeer = useCallback(async (publicKey) => {
    await api.blockPeer(publicKey);
    poll();
  }, [poll]);

  const dropSession = useCallback(async (sessionId) => {
    await api.dropSession(sessionId);
    poll();
  }, [poll]);

  const sendFile = useCallback(async (file, target) => {
    await api.sendFile(file, target);
    poll();
  }, [poll]);

  const sendMessage = useCallback(async (toPubkey, text) => {
    await api.sendMessage(toPubkey, text);
  }, []);

  const clearCache = useCallback(async () => {
    await api.clearCache();
    poll();
  }, [poll]);

  const submitComputeTask = useCallback(async (peer, payload) => {
    await api.submitComputeTask(peer, payload);
    poll();
  }, [poll]);

  const value = {
    ...state,
    connected,
    trustPeer,
    blockPeer,
    dropSession,
    sendFile,
    sendMessage,
    clearCache,
    submitComputeTask,
    refresh: poll,
  };

  return (
    <DaemonContext.Provider value={value}>
      {children}
    </DaemonContext.Provider>
  );
}
