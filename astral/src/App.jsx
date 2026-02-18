import { useState, useEffect, useRef } from "react";

// ─── API Client ───────────────────────────────────────────────────────────────

const API_BASE = 'http://127.0.0.1:9001';

const api = {
  getStatus: () => fetch(`${API_BASE}/status`).then(r => r.json()),
  getPeers: () => fetch(`${API_BASE}/peers`).then(r => r.json()),
  getTrust: () => fetch(`${API_BASE}/trust`).then(r => r.json()),
  trustPeer: (publicKey) => fetch(`${API_BASE}/trust/add`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ public_key: publicKey })
  }).then(r => r.json()),
  blockPeer: (publicKey) => fetch(`${API_BASE}/trust/block`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ public_key: publicKey })
  }).then(r => r.json()),
  dropSession: (sessionId) => fetch(`${API_BASE}/sessions/${sessionId}`, {
    method: 'DELETE'
  }).then(r => r.json()),
  inspectSession: (sessionId) => fetch(`${API_BASE}/sessions/${sessionId}`).then(r => r.json()),
  sendFile: async (file, target) => {
    const formData = new FormData();
    formData.append('file', file);
    if (target) formData.append('target', JSON.stringify(target));
    return fetch(`${API_BASE}/send`, { method: 'POST', body: formData }).then(r => r.json());
  },
  getFiles: () => fetch(`${API_BASE}/files`).then(r => r.json()),
  getCacheStats: () => fetch(`${API_BASE}/cache`).then(r => r.json()),
  getSchemas: () => fetch(`${API_BASE}/schema`).then(r => r.json()),
};

// ─── Utilities ────────────────────────────────────────────────────────────────

function rssiToStrength(rssi) {
  if (rssi > -50) return 4;
  if (rssi > -65) return 3;
  if (rssi > -75) return 2;
  return 1;
}

function timeAgo(secs) {
  if (secs < 60) return `${secs}s ago`;
  if (secs < 3600) return `${Math.floor(secs / 60)}m ago`;
  return `${Math.floor(secs / 3600)}h ago`;
}

function shortKey(key) {
  return key?.slice(0, 8) || 'unknown';
}

// ─── Signal Strength Icon ─────────────────────────────────────────────────────

function SignalBars({ rssi, color = "#f5a623" }) {
  const strength = rssiToStrength(rssi);
  return (
    <svg width="16" height="12" viewBox="0 0 16 12" fill="none">
    {[1, 2, 3, 4].map((bar) => (
      <rect
      key={bar}
      x={(bar - 1) * 4}
      y={12 - bar * 3}
      width="3"
      height={bar * 3}
      fill={bar <= strength ? color : "rgba(255,255,255,0.15)"}
      rx="0.5"
      />
    ))}
    </svg>
  );
}

// ─── Pulse Dot ────────────────────────────────────────────────────────────────

function PulseDot({ color = "#f5a623", size = 8 }) {
  return (
    <span style={{ position: "relative", display: "inline-block", width: size, height: size }}>
    <span style={{
      position: "absolute", inset: 0, borderRadius: "50%",
      background: color, opacity: 0.3,
      animation: "pulseRing 2s ease-out infinite",
    }} />
    <span style={{
      position: "absolute", inset: "2px", borderRadius: "50%",
      background: color,
    }} />
    </span>
  );
}

// ─── Lock Icon ────────────────────────────────────────────────────────────────

function LockIcon({ size = 10 }) {
  return (
    <svg width={size} height={size + 2} viewBox="0 0 10 12" fill="none">
    <rect x="1" y="5" width="8" height="7" rx="1" fill="currentColor" opacity="0.6" />
    <path d="M3 5V3.5a2 2 0 014 0V5" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" opacity="0.6" />
    </svg>
  );
}

// ─── Nav Bar ──────────────────────────────────────────────────────────────────

function NavBar({ active, onNav }) {
  const tabs = [
    { id: "nearby", label: "Nearby", icon: "⬡" },
    { id: "sessions", label: "Sessions", icon: "≡" },
    { id: "files", label: "Files", icon: "◈" },
    { id: "system", label: "System", icon: "◎" },
  ];
  return (
    <nav style={{
      display: "flex", borderTop: "1px solid rgba(245,166,35,0.15)",
          background: "#0a0a08",
    }}>
    {tabs.map(t => (
      <button key={t.id} onClick={() => onNav(t.id)} style={{
        flex: 1, padding: "10px 4px 8px", border: "none", background: "none",
        cursor: "pointer", display: "flex", flexDirection: "column",
        alignItems: "center", gap: 3,
        color: active === t.id ? "#f5a623" : "rgba(255,255,255,0.3)",
                    transition: "color 0.15s",
      }}>
      <span style={{ fontSize: 18, lineHeight: 1 }}>{t.icon}</span>
      <span style={{ fontSize: 9, fontFamily: "'Space Mono', monospace", letterSpacing: "0.08em", textTransform: "uppercase" }}>{t.label}</span>
      {active === t.id && (
        <span style={{ position: "absolute", bottom: 0, width: 24, height: 2, background: "#f5a623", borderRadius: 1 }} />
      )}
      </button>
    ))}
    </nav>
  );
}

// ─── Nearby Screen ────────────────────────────────────────────────────────────

function NearbyScreen({ peers, trust, onTrust, onBlock }) {
  const trustMap = {};
  trust.forEach(t => trustMap[t.public_key] = t.level);

  return (
    <div style={{ flex: 1, overflow: "auto", padding: "16px 12px" }}>
    <SectionHeader label="Discovered Peers" count={peers.length} accent="#f5a623" />

    {peers.length === 0 ? (
      <div style={{
        padding: "20px",
        textAlign: "center",
        fontFamily: "'Space Mono', monospace",
        fontSize: 11,
        color: "rgba(255,255,255,0.3)"
      }}>
      No peers discovered yet...
      </div>
    ) : (
      <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
      {peers.map(peer => (
        <PeerCard
        key={peer.public_key}
        peer={peer}
        trustLevel={trustMap[peer.public_key] || 'Untrusted'}
        onTrust={() => onTrust(peer.public_key)}
        onBlock={() => onBlock(peer.public_key)}
        />
      ))}
      </div>
    )}
    </div>
  );
}

function SectionHeader({ label, count, accent }) {
  return (
    <div style={{ display: "flex", alignItems: "center", gap: 8, marginBottom: 10 }}>
    <span style={{
      fontFamily: "'Space Mono', monospace", fontSize: 9,
      color: accent, letterSpacing: "0.15em", textTransform: "uppercase",
    }}>{label}</span>
    {count !== null && (
      <span style={{
        fontFamily: "'Space Mono', monospace", fontSize: 9,
        color: "rgba(255,255,255,0.25)", background: "rgba(255,255,255,0.06)",
                        padding: "1px 6px", borderRadius: 3,
      }}>{count}</span>
    )}
    <div style={{ flex: 1, height: 1, background: `${accent}22` }} />
    </div>
  );
}

function PeerCard({ peer, trustLevel, onTrust, onBlock }) {
  const isActive = peer.last_seen_secs < 5;
  const isTrusted = trustLevel === 'Trusted';
  const isBlocked = trustLevel === 'Blocked';

  return (
    <div style={{
      background: "rgba(245,166,35,0.04)",
          border: `1px solid ${isActive ? "rgba(245,166,35,0.2)" : "rgba(255,255,255,0.07)"}`,
          borderRadius: 8,
          padding: "10px 12px",
          display: "flex",
          flexDirection: "column",
          gap: 6,
    }}>
    <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between" }}>
    <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
    {isActive
      ? <PulseDot color="#f5a623" size={8} />
      : <span style={{ width: 8, height: 8, borderRadius: "50%", background: "rgba(255,255,255,0.2)", display: "inline-block" }} />
    }
    <span style={{ fontFamily: "'Space Mono', monospace", fontSize: 13, color: "#f0ead6", fontWeight: 700 }}>
    {shortKey(peer.public_key)}
    </span>
    {isTrusted && (
      <span style={{
        fontSize: 9, fontFamily: "'Space Mono', monospace",
        color: "#4a9e6b", background: "rgba(74,158,107,0.12)",
                   padding: "1px 5px", borderRadius: 3, letterSpacing: "0.1em",
      }}>trusted</span>
    )}
    {isBlocked && (
      <span style={{
        fontSize: 9, fontFamily: "'Space Mono', monospace",
        color: "#e05c5c", background: "rgba(224,92,92,0.12)",
                   padding: "1px 5px", borderRadius: 3, letterSpacing: "0.1em",
      }}>blocked</span>
    )}
    </div>
    </div>

    <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between" }}>
    <span style={{
      fontFamily: "'Space Mono', monospace", fontSize: 10,
      color: "rgba(255,255,255,0.3)", letterSpacing: "0.08em",
    }}>
    {peer.addr}
    </span>
    <span style={{ fontSize: 9, fontFamily: "'Space Mono', monospace", color: "rgba(255,255,255,0.25)" }}>
    {isActive ? "active" : timeAgo(peer.last_seen_secs)}
    </span>
    </div>

    {peer.buffered_chunks > 0 && (
      <div style={{
        fontSize: 9,
        fontFamily: "'Space Mono', monospace",
        color: "rgba(245,166,35,0.6)",
                                  background: "rgba(245,166,35,0.08)",
                                  padding: "3px 6px",
                                  borderRadius: 3,
      }}>
      {peer.buffered_chunks} chunks buffered (trust to receive)
      </div>
    )}

    {!isTrusted && !isBlocked && (
      <div style={{ display: "flex", gap: 6, marginTop: 4 }}>
      <button onClick={onTrust} style={{
        flex: 1,
        background: "rgba(74,158,107,0.15)",
                                  border: "1px solid rgba(74,158,107,0.3)",
                                  borderRadius: 4,
                                  padding: "5px 8px",
                                  color: "#4a9e6b",
                                  fontFamily: "'Space Mono', monospace",
                                  fontSize: 9,
                                  cursor: "pointer",
      }}>Trust</button>
      <button onClick={onBlock} style={{
        flex: 1,
        background: "rgba(224,92,92,0.15)",
                                  border: "1px solid rgba(224,92,92,0.3)",
                                  borderRadius: 4,
                                  padding: "5px 8px",
                                  color: "#e05c5c",
                                  fontFamily: "'Space Mono', monospace",
                                  fontSize: 9,
                                  cursor: "pointer",
      }}>Block</button>
      </div>
    )}
    </div>
  );
}

// ─── Sessions Screen ──────────────────────────────────────────────────────────

function SessionsScreen({ sessions, onDropSession }) {
  return (
    <div style={{ flex: 1, overflow: "auto", padding: "16px 12px" }}>
    <SectionHeader label="Active Sessions" count={sessions.length} accent="#f5a623" />

    {sessions.length === 0 ? (
      <div style={{
        padding: "20px",
        textAlign: "center",
        fontFamily: "'Space Mono', monospace",
        fontSize: 11,
        color: "rgba(255,255,255,0.3)"
      }}>
      No active sessions
      </div>
    ) : (
      <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
      {sessions.map(session => (
        <SessionCard key={session.session_id} session={session} onDrop={onDropSession} />
      ))}
      </div>
    )}
    </div>
  );
}

function SessionCard({ session, onDrop }) {
  return (
    <div style={{
      background: "rgba(245,166,35,0.04)",
          border: "1px solid rgba(245,166,35,0.15)",
          borderRadius: 8,
          padding: "10px 12px",
          display: "flex",
          flexDirection: "column",
          gap: 6,
    }}>
    <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between" }}>
    <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
    <PulseDot color="#4a9e6b" size={8} />
    <span style={{ fontFamily: "'Space Mono', monospace", fontSize: 13, color: "#f0ead6", fontWeight: 700 }}>
    {shortKey(session.peer_pubkey)}
    </span>
    <LockIcon size={10} />
    </div>
    <span style={{
      fontSize: 9,
      fontFamily: "'Space Mono', monospace",
      color: "rgba(245,166,35,0.5)",
          background: "rgba(245,166,35,0.08)",
          padding: "2px 6px",
          borderRadius: 3,
    }}>{session.contract}</span>
    </div>

    <div style={{ display: "flex", alignItems: "center", gap: 12, fontSize: 10, fontFamily: "'Space Mono', monospace", color: "rgba(255,255,255,0.4)" }}>
    <span>session: {shortKey(session.session_id)}</span>
    <span>uptime: {session.established_secs}s</span>
    </div>

    <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between", marginTop: 4 }}>
    <span style={{ fontSize: 9, fontFamily: "'Space Mono', monospace", color: "rgba(255,255,255,0.3)" }}>
    {session.peer}
    </span>
    <button onClick={() => onDrop(session.session_id)} style={{
      background: "rgba(224,92,92,0.15)",
          border: "1px solid rgba(224,92,92,0.3)",
          borderRadius: 4,
          padding: "4px 8px",
          color: "#e05c5c",
          fontFamily: "'Space Mono', monospace",
          fontSize: 9,
          cursor: "pointer",
    }}>Drop</button>
    </div>
    </div>
  );
}

// ─── Files Screen ─────────────────────────────────────────────────────────────

function FilesScreen({ files, onSendFile }) {
  const fileInputRef = useRef(null);

  const handleFileSelect = (e) => {
    const file = e.target.files[0];
    if (file) {
      onSendFile(file, { type: 'broadcast' });
    }
  };

  return (
    <div style={{ flex: 1, overflow: "auto", padding: "16px 12px" }}>
    <SectionHeader label="File Transfer" count={null} accent="#f5a623" />

    <div style={{ marginBottom: 20 }}>
    <input
    ref={fileInputRef}
    type="file"
    onChange={handleFileSelect}
    style={{ display: 'none' }}
    />
    <button onClick={() => fileInputRef.current?.click()} style={{
      width: "100%",
      background: "rgba(245,166,35,0.15)",
          border: "1px solid rgba(245,166,35,0.3)",
          borderRadius: 6,
          padding: "12px",
          color: "#f5a623",
          fontFamily: "'Space Mono', monospace",
          fontSize: 12,
          cursor: "pointer",
          display: "flex",
          alignItems: "center",
          justifyContent: "center",
          gap: 8,
    }}>
    <span style={{ fontSize: 16 }}>+</span>
    Send File (Broadcast)
    </button>
    </div>

    {files.received && files.received.length > 0 && (
      <>
      <SectionHeader label="Received Files" count={files.received.length} accent="#4a9e6b" />
      <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
      {files.received.map((filename, i) => (
        <div key={i} style={{
          background: "rgba(74,158,107,0.08)",
                                            border: "1px solid rgba(74,158,107,0.2)",
                                            borderRadius: 6,
                                            padding: "8px 10px",
                                            fontFamily: "'Space Mono', monospace",
                                            fontSize: 11,
                                            color: "#4a9e6b",
        }}>
        {filename}
        </div>
      ))}
      </div>
      </>
    )}

    {files.in_progress && files.in_progress.length > 0 && (
      <div style={{ marginTop: 20 }}>
      <SectionHeader label="In Progress" count={files.in_progress.length} accent="#f5a623" />
      <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
      {files.in_progress.map((filename, i) => (
        <div key={i} style={{
          background: "rgba(245,166,35,0.08)",
                                               border: "1px solid rgba(245,166,35,0.2)",
                                               borderRadius: 6,
                                               padding: "8px 10px",
                                               fontFamily: "'Space Mono', monospace",
                                               fontSize: 11,
                                               color: "#f5a623",
                                               display: "flex",
                                               alignItems: "center",
                                               gap: 8,
        }}>
        <PulseDot color="#f5a623" size={6} />
        {filename}
        </div>
      ))}
      </div>
      </div>
    )}
    </div>
  );
}

// ─── System Screen ────────────────────────────────────────────────────────────

function SystemScreen({ status, cache, schemas }) {
  return (
    <div style={{ flex: 1, overflow: "auto", padding: "16px 12px" }}>
    <SectionHeader label="Daemon Status" count={null} accent="#f5a623" />

    <div style={{ marginBottom: 20, padding: "12px", borderRadius: 8, background: "rgba(255,255,255,0.02)", border: "1px solid rgba(255,255,255,0.07)" }}>
    <StatRow label="Active Sessions" value={status.sessions?.length || 0} />
    <StatRow label="Peers Discovered" value={status.peers_discovered || 0} />
    </div>

    <SectionHeader label="Chunk Cache" count={null} accent="#5b8dd9" />

    <div style={{ marginBottom: 20, padding: "12px", borderRadius: 8, background: "rgba(91,141,217,0.04)", border: "1px solid rgba(91,141,217,0.15)" }}>
    <StatRow label="Chunks Cached" value={cache.chunks || 0} />
    <StatRow label="Bytes Cached" value={`${Math.floor((cache.bytes || 0) / 1024)} KB`} />
    </div>

    <SectionHeader label="Schemas" count={schemas.length} accent="#9b7fd4" />

    <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
    {schemas.map(schema => (
      <div key={schema.id} style={{
        background: "rgba(155,127,212,0.04)",
                            border: "1px solid rgba(155,127,212,0.15)",
                            borderRadius: 6,
                            padding: "8px 10px",
                            display: "flex",
                            flexDirection: "column",
                            gap: 4,
      }}>
      <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between" }}>
      <span style={{ fontFamily: "'Space Mono', monospace", fontSize: 11, color: "#9b7fd4", fontWeight: 700 }}>
      {schema.name}
      </span>
      <span style={{ fontFamily: "'Space Mono', monospace", fontSize: 9, color: "rgba(155,127,212,0.5)" }}>
      tag {schema.type_tag}
      </span>
      </div>
      <span style={{ fontFamily: "'Space Mono', monospace", fontSize: 9, color: "rgba(255,255,255,0.3)" }}>
      {shortKey(schema.id)}...
      </span>
      </div>
    ))}
    </div>
    </div>
  );
}

function StatRow({ label, value }) {
  return (
    <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center", marginBottom: 8 }}>
    <span style={{ fontFamily: "'Space Mono', monospace", fontSize: 10, color: "rgba(255,255,255,0.4)" }}>
    {label}
    </span>
    <span style={{ fontFamily: "'Space Mono', monospace", fontSize: 12, color: "#f0ead6", fontWeight: 700 }}>
    {value}
    </span>
    </div>
  );
}

// ─── App Root ─────────────────────────────────────────────────────────────────

export default function App() {
  const [tab, setTab] = useState("nearby");
  const [status, setStatus] = useState({ sessions: [], peers_discovered: 0, cache: { chunks: 0, bytes: 0 } });
  const [peers, setPeers] = useState([]);
  const [trust, setTrust] = useState([]);
  const [files, setFiles] = useState({ received: [], in_progress: [] });
  const [cache, setCache] = useState({ chunks: 0, bytes: 0 });
  const [schemas, setSchemas] = useState([]);
  const [error, setError] = useState(null);

  // Polling interval
  useEffect(() => {
    const fetchData = async () => {
      try {
        const [statusData, peersData, trustData, filesData, cacheData, schemasData] = await Promise.all([
          api.getStatus().catch(() => ({ sessions: [], peers_discovered: 0, cache: { chunks: 0, bytes: 0 } })),
                                                                                                        api.getPeers().catch(() => ({ peers: [] })),
                                                                                                        api.getTrust().catch(() => ({ rules: [] })),
                                                                                                        api.getFiles().catch(() => ({ received: [], in_progress: [] })),
                                                                                                        api.getCacheStats().catch(() => ({ chunks: 0, bytes: 0 })),
                                                                                                        api.getSchemas().catch(() => ({ schemas: [] })),
        ]);

        setStatus(statusData);
        setPeers(peersData.peers || []);
        setTrust(trustData.rules || []);
        setFiles(filesData);
        setCache(cacheData);
        setSchemas(schemasData.schemas || []);
        setError(null);
      } catch (err) {
        setError(err.message);
      }
    };

    fetchData();
    const interval = setInterval(fetchData, 2000); // Poll every 2s
    return () => clearInterval(interval);
  }, []);

  const handleTrust = async (publicKey) => {
    try {
      await api.trustPeer(publicKey);
      // Refresh trust list
      const trustData = await api.getTrust();
      setTrust(trustData.rules || []);
    } catch (err) {
      console.error('Trust failed:', err);
    }
  };

  const handleBlock = async (publicKey) => {
    try {
      await api.blockPeer(publicKey);
      const trustData = await api.getTrust();
      setTrust(trustData.rules || []);
    } catch (err) {
      console.error('Block failed:', err);
    }
  };

  const handleDropSession = async (sessionId) => {
    try {
      await api.dropSession(sessionId);
      const statusData = await api.getStatus();
      setStatus(statusData);
    } catch (err) {
      console.error('Drop session failed:', err);
    }
  };

  const handleSendFile = async (file, target) => {
    try {
      await api.sendFile(file, target);
      const filesData = await api.getFiles();
      setFiles(filesData);
    } catch (err) {
      console.error('Send file failed:', err);
    }
  };

  const renderMain = () => {
    switch (tab) {
      case "nearby":
        return <NearbyScreen peers={peers} trust={trust} onTrust={handleTrust} onBlock={handleBlock} />;
      case "sessions":
        return <SessionsScreen sessions={status.sessions || []} onDropSession={handleDropSession} />;
      case "files":
        return <FilesScreen files={files} onSendFile={handleSendFile} />;
      case "system":
        return <SystemScreen status={status} cache={cache} schemas={schemas} />;
      default:
        return <NearbyScreen peers={peers} trust={trust} onTrust={handleTrust} onBlock={handleBlock} />;
    }
  };

  return (
    <>
    <style>{`
      @import url('https://fonts.googleapis.com/css2?family=Space+Mono:wght@400;700&display=swap');
      * { box-sizing: border-box; margin: 0; padding: 0; }
      body { background: #0a0a08; }
      @keyframes pulseRing {
        0% { transform: scale(1); opacity: 0.4; }
        70% { transform: scale(2.4); opacity: 0; }
        100% { transform: scale(2.4); opacity: 0; }
      }
      ::-webkit-scrollbar { width: 4px; }
      ::-webkit-scrollbar-track { background: transparent; }
      ::-webkit-scrollbar-thumb { background: rgba(245,166,35,0.2); border-radius: 2px; }
      `}</style>

      <div style={{
        width: "100%", maxWidth: 420, height: "100vh", margin: "0 auto",
        background: "#0d0d0b",
        display: "flex", flexDirection: "column",
        fontFamily: "'Space Mono', monospace",
        position: "relative", overflow: "hidden",
        boxShadow: "0 0 80px rgba(245,166,35,0.05)",
      }}>
      {/* Header */}
      <div style={{
        padding: "14px 16px 10px",
        borderBottom: "1px solid rgba(245,166,35,0.12)",
          background: "rgba(0,0,0,0.4)",
          display: "flex", alignItems: "center", justifyContent: "space-between",
      }}>
      <div>
      <div style={{ fontFamily: "'Space Mono', monospace", fontSize: 16, fontWeight: 700, color: "#f5a623", letterSpacing: "0.05em" }}>
      ASTRAL
      </div>
      <div style={{ fontFamily: "'Space Mono', monospace", fontSize: 8, color: "rgba(255,255,255,0.25)", letterSpacing: "0.15em", marginTop: 1 }}>
      SUMMIT PROTOCOL · PROXIMAL MESH
      </div>
      </div>
      <div style={{ display: "flex", alignItems: "center", gap: 6 }}>
      {error ? (
        <>
        <span style={{ width: 7, height: 7, borderRadius: "50%", background: "#e05c5c" }} />
        <span style={{ fontFamily: "'Space Mono', monospace", fontSize: 9, color: "#e05c5c" }}>disconnected</span>
        </>
      ) : (
        <>
        <PulseDot color="#4a9e6b" size={7} />
        <span style={{ fontFamily: "'Space Mono', monospace", fontSize: 9, color: "#4a9e6b" }}>live</span>
        </>
      )}
      </div>
      </div>

      {/* Main content */}
      <div style={{ flex: 1, display: "flex", flexDirection: "column", overflow: "hidden" }}>
      {renderMain()}
      </div>

      {/* Nav */}
      <div style={{ position: "relative" }}>
      <NavBar active={tab} onNav={setTab} />
      </div>
      </div>
      </>
  );
}
