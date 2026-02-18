import { useState, useEffect, useRef } from "react";

// ─── Fake Data ────────────────────────────────────────────────────────────────

const PEERS = [
  { id: "3a9fc421", name: "rpi-werkbank", key: "3a9fc421e0b17d2a", trusted: true, rssi: -42, protocol: "summit", lastSeen: 0, schemas: ["message", "post", "voice"] },
  { id: "7f2e1b09", name: "nova-laptop", key: "7f2e1b09c3a8f651", trusted: true, rssi: -61, protocol: "summit", lastSeen: 2000, schemas: ["message", "post"] },
  { id: "c8a30d44", name: "unnamed-pi", key: "c8a30d44b2190fe3", trusted: false, rssi: -74, protocol: "summit", lastSeen: 8000, schemas: ["message"] },
];

const AMBIENT = [
  { id: "mdns-1", label: "MacBook Pro", type: "mdns", service: "_airdrop._tcp", rssi: -55 },
  { id: "mdns-2", label: "HP LaserJet", type: "mdns", service: "_printer._tcp", rssi: -68 },
  { id: "ble-1", label: "BLE Device", type: "ble", manufacturer: "Apple Inc.", rssi: -71 },
  { id: "ble-2", label: "BLE Beacon", type: "ble", manufacturer: "Unknown", rssi: -83 },
  { id: "wifi-1", label: "WiFi Ghost", type: "wifi", rssi: -77 },
  { id: "wifi-2", label: "WiFi Ghost", type: "wifi", rssi: -89 },
  { id: "wifi-3", label: "WiFi Ghost", type: "wifi", rssi: -91 },
];

const CHANNELS = [
  { id: "general", name: "general", unread: 3, members: 3 },
  { id: "hardware", name: "hardware", unread: 0, members: 2 },
  { id: "bench-log", name: "bench-log", unread: 1, members: 3 },
];

const DM_MESSAGES = [
  { id: 1, sender: "rpi-werkbank", senderKey: "3a9fc421", text: "handshake complete — session up", ts: "14:22:01", encrypted: true, mine: false },
  { id: 2, sender: "me", senderKey: "self", text: "got it. sending test chunk now", ts: "14:22:14", encrypted: true, mine: true },
  { id: 3, sender: "rpi-werkbank", senderKey: "3a9fc421", text: "received. schema negotiation looks good", ts: "14:22:19", encrypted: true, mine: false },
  { id: 4, sender: "me", senderKey: "self", text: "running the voice test next", ts: "14:22:31", encrypted: true, mine: true },
  { id: 5, sender: "rpi-werkbank", senderKey: "3a9fc421", text: "ready", ts: "14:22:33", encrypted: true, mine: false },
];

const PUBLIC_POSTS = [
  { id: 1, sender: "nova-laptop", senderKey: "7f2e1b09", topic: "general", text: "setting up the bench — anyone nearby on summit?", ts: "14:18:44", type: "post" },
  { id: 2, sender: "rpi-werkbank", senderKey: "3a9fc421", topic: "music", text: "streaming ambient tonight — tune in on audio_stream", ts: "14:20:11", type: "stream_announce" },
  { id: 3, sender: "unnamed-pi", senderKey: "c8a30d44", topic: "alert", text: "core temp 81°C — throttling", ts: "14:21:55", type: "alert" },
];

const CHANNEL_MESSAGES = {
  general: [
    { id: 1, sender: "nova-laptop", senderKey: "7f2e1b09", text: "anyone tested the chunk cache yet?", ts: "14:15:02", mine: false },
    { id: 2, sender: "rpi-werkbank", senderKey: "3a9fc421", text: "yes — zero-copy hits working on tmpfs", ts: "14:15:44", mine: false },
    { id: 3, sender: "me", senderKey: "self", text: "nice. what's the latency delta vs malloc?", ts: "14:16:01", mine: true },
    { id: 4, sender: "rpi-werkbank", senderKey: "3a9fc421", text: "~40μs vs ~180μs on the Pi. consistent.", ts: "14:16:22", mine: false },
    { id: 5, sender: "nova-laptop", senderKey: "7f2e1b09", text: "that's the number. worth the complexity", ts: "14:17:01", mine: false },
  ],
  hardware: [
    { id: 1, sender: "rpi-werkbank", senderKey: "3a9fc421", text: "crossover cable test done — auto-MDIX works", ts: "14:10:11", mine: false },
    { id: 2, sender: "me", senderKey: "self", text: "good. no config needed?", ts: "14:10:44", mine: true },
    { id: 3, sender: "rpi-werkbank", senderKey: "3a9fc421", text: "none. link-local up in ~3s", ts: "14:11:02", mine: false },
  ],
  "bench-log": [
    { id: 1, sender: "nova-laptop", senderKey: "7f2e1b09", text: "session #4 — noise_xx complete in 2.1ms", ts: "14:19:33", mine: false },
  ],
};

// ─── Utilities ────────────────────────────────────────────────────────────────

function rssiToStrength(rssi) {
  if (rssi > -50) return 4;
  if (rssi > -65) return 3;
  if (rssi > -75) return 2;
  return 1;
}

function rssiToLabel(rssi) {
  if (rssi > -50) return "strong";
  if (rssi > -65) return "good";
  if (rssi > -75) return "fair";
  return "weak";
}

function timeAgo(ms) {
  if (ms < 1000) return "now";
  if (ms < 60000) return `${Math.floor(ms / 1000)}s ago`;
  return `${Math.floor(ms / 60000)}m ago`;
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
    { id: "channels", label: "Channels", icon: "≡" },
    { id: "public", label: "Public", icon: "◎" },
    { id: "profile", label: "Profile", icon: "◈" },
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

function NearbyScreen({ onOpenDM }) {
  const [tick, setTick] = useState(0);
  useEffect(() => {
    const t = setInterval(() => setTick(n => n + 1), 1000);
    return () => clearInterval(t);
  }, []);

  return (
    <div style={{ flex: 1, overflow: "auto", padding: "16px 12px" }}>
      {/* Section: Summit Peers */}
      <div style={{ marginBottom: 24 }}>
        <SectionHeader label="Summit Peers" count={PEERS.length} accent="#f5a623" />
        <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
          {PEERS.map(peer => (
            <PeerCard key={peer.id} peer={peer} tick={tick} onTap={() => onOpenDM(peer)} />
          ))}
        </div>
      </div>

      {/* Section: Ambient Signals */}
      <div>
        <SectionHeader label="Ambient Signals" count={AMBIENT.length} accent="#4a9e6b" />
        <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
          {AMBIENT.map(dev => (
            <AmbientRow key={dev.id} device={dev} />
          ))}
        </div>
      </div>
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
      <span style={{
        fontFamily: "'Space Mono', monospace", fontSize: 9,
        color: "rgba(255,255,255,0.25)", background: "rgba(255,255,255,0.06)",
        padding: "1px 6px", borderRadius: 3,
      }}>{count}</span>
      <div style={{ flex: 1, height: 1, background: `${accent}22` }} />
    </div>
  );
}

function PeerCard({ peer, tick, onTap }) {
  const isActive = peer.lastSeen < 3000;
  return (
    <div onClick={onTap} style={{
      background: "rgba(245,166,35,0.04)", border: `1px solid ${isActive ? "rgba(245,166,35,0.2)" : "rgba(255,255,255,0.07)"}`,
      borderRadius: 8, padding: "10px 12px", cursor: "pointer",
      transition: "all 0.15s",
      display: "flex", flexDirection: "column", gap: 6,
    }}>
      <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between" }}>
        <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
          {isActive
            ? <PulseDot color="#f5a623" size={8} />
            : <span style={{ width: 8, height: 8, borderRadius: "50%", background: "rgba(255,255,255,0.2)", display: "inline-block" }} />
          }
          <span style={{ fontFamily: "'Space Mono', monospace", fontSize: 13, color: "#f0ead6", fontWeight: 700 }}>
            {peer.name}
          </span>
          {peer.trusted && (
            <span style={{
              fontSize: 9, fontFamily: "'Space Mono', monospace",
              color: "#4a9e6b", background: "rgba(74,158,107,0.12)",
              padding: "1px 5px", borderRadius: 3, letterSpacing: "0.1em",
            }}>trusted</span>
          )}
        </div>
        <SignalBars rssi={peer.rssi} />
      </div>

      <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between" }}>
        <span style={{
          fontFamily: "'Space Mono', monospace", fontSize: 10,
          color: "rgba(255,255,255,0.3)", letterSpacing: "0.08em",
        }}>
          {peer.key}
        </span>
        <span style={{ fontSize: 9, fontFamily: "'Space Mono', monospace", color: "rgba(255,255,255,0.25)" }}>
          {isActive ? "active" : timeAgo(peer.lastSeen + tick * 1000)}
        </span>
      </div>

      <div style={{ display: "flex", gap: 4, flexWrap: "wrap" }}>
        {peer.schemas.map(s => (
          <span key={s} style={{
            fontSize: 8, fontFamily: "'Space Mono', monospace",
            color: "rgba(245,166,35,0.5)", background: "rgba(245,166,35,0.06)",
            padding: "1px 5px", borderRadius: 2, letterSpacing: "0.06em",
          }}>{s}</span>
        ))}
      </div>
    </div>
  );
}

function AmbientRow({ device }) {
  const colors = { mdns: "#5b8dd9", ble: "#9b7fd4", wifi: "rgba(255,255,255,0.2)" };
  const color = colors[device.type];
  return (
    <div style={{
      display: "flex", alignItems: "center", gap: 10,
      padding: "7px 10px", borderRadius: 6,
      background: "rgba(255,255,255,0.02)", border: "1px solid rgba(255,255,255,0.05)",
    }}>
      <span style={{ width: 6, height: 6, borderRadius: "50%", background: color, flexShrink: 0 }} />
      <span style={{ fontFamily: "'Space Mono', monospace", fontSize: 11, color: "rgba(255,255,255,0.45)", flex: 1 }}>
        {device.label}
      </span>
      {device.service && (
        <span style={{ fontSize: 9, fontFamily: "'Space Mono', monospace", color: "rgba(91,141,217,0.6)" }}>
          {device.service}
        </span>
      )}
      {device.manufacturer && (
        <span style={{ fontSize: 9, fontFamily: "'Space Mono', monospace", color: "rgba(155,127,212,0.6)" }}>
          {device.manufacturer}
        </span>
      )}
      <SignalBars rssi={device.rssi} color={color} />
    </div>
  );
}

// ─── Channels Screen ──────────────────────────────────────────────────────────

function ChannelsScreen({ onOpenChannel }) {
  return (
    <div style={{ flex: 1, overflow: "auto", padding: "16px 12px" }}>
      <SectionHeader label="Channels" count={CHANNELS.length} accent="#f5a623" />
      <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
        {CHANNELS.map(ch => (
          <div key={ch.id} onClick={() => onOpenChannel(ch)} style={{
            display: "flex", alignItems: "center", gap: 10,
            padding: "11px 12px", borderRadius: 8, cursor: "pointer",
            background: "rgba(245,166,35,0.03)", border: "1px solid rgba(245,166,35,0.12)",
            transition: "all 0.15s",
          }}>
            <span style={{ fontFamily: "'Space Mono', monospace", fontSize: 13, color: "rgba(245,166,35,0.5)" }}>#</span>
            <span style={{ fontFamily: "'Space Mono', monospace", fontSize: 13, color: "#f0ead6", flex: 1 }}>{ch.name}</span>
            <span style={{ fontSize: 9, fontFamily: "'Space Mono', monospace", color: "rgba(255,255,255,0.25)" }}>
              {ch.members} peers
            </span>
            {ch.unread > 0 && (
              <span style={{
                minWidth: 18, height: 18, borderRadius: 9,
                background: "#f5a623", color: "#0a0a08",
                fontFamily: "'Space Mono', monospace", fontSize: 9, fontWeight: 700,
                display: "flex", alignItems: "center", justifyContent: "center", padding: "0 4px",
              }}>{ch.unread}</span>
            )}
          </div>
        ))}
      </div>

      {/* DMs section */}
      <div style={{ marginTop: 24 }}>
        <SectionHeader label="Direct Messages" count={PEERS.filter(p => p.trusted).length} accent="#f5a623" />
        <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
          {PEERS.filter(p => p.trusted).map(peer => (
            <div key={peer.id} style={{
              display: "flex", alignItems: "center", gap: 10,
              padding: "9px 12px", borderRadius: 8,
              background: "rgba(255,255,255,0.02)", border: "1px solid rgba(255,255,255,0.06)",
            }}>
              <PulseDot color="#f5a623" size={6} />
              <span style={{ fontFamily: "'Space Mono', monospace", fontSize: 12, color: "#f0ead6", flex: 1 }}>{peer.name}</span>
              <span style={{ fontFamily: "'Space Mono', monospace", fontSize: 9, color: "rgba(255,255,255,0.2)" }}>{peer.id}</span>
            </div>
          ))}
        </div>
      </div>
    </div>
  );
}

// ─── Conversation View ────────────────────────────────────────────────────────

function ConversationView({ target, messages, onBack }) {
  const [input, setInput] = useState("");
  const bottomRef = useRef(null);

  useEffect(() => {
    bottomRef.current?.scrollIntoView({ behavior: "smooth" });
  }, [messages]);

  const isChannel = target.id && CHANNELS.find(c => c.id === target.id);

  return (
    <div style={{ flex: 1, display: "flex", flexDirection: "column", overflow: "hidden" }}>
      {/* Header */}
      <div style={{
        padding: "10px 12px", borderBottom: "1px solid rgba(245,166,35,0.12)",
        display: "flex", alignItems: "center", gap: 10,
        background: "rgba(0,0,0,0.3)",
      }}>
        <button onClick={onBack} style={{
          background: "none", border: "none", color: "rgba(245,166,35,0.6)",
          fontFamily: "'Space Mono', monospace", fontSize: 13, cursor: "pointer", padding: 0,
        }}>←</button>
        <span style={{ fontFamily: "'Space Mono', monospace", fontSize: 13, color: "#f0ead6", fontWeight: 700 }}>
          {isChannel ? `#${target.name}` : target.name}
        </span>
        {!isChannel && (
          <span style={{ fontFamily: "'Space Mono', monospace", fontSize: 9, color: "rgba(255,255,255,0.25)" }}>
            {target.key}
          </span>
        )}
        <div style={{ flex: 1 }} />
        <LockIcon size={10} />
        <span style={{ fontSize: 9, fontFamily: "'Space Mono', monospace", color: "rgba(245,166,35,0.4)" }}>e2e</span>
      </div>

      {/* Messages */}
      <div style={{ flex: 1, overflow: "auto", padding: "12px", display: "flex", flexDirection: "column", gap: 8 }}>
        {messages.map(msg => (
          <MessageBubble key={msg.id} msg={msg} />
        ))}
        <div ref={bottomRef} />
      </div>

      {/* Input */}
      <div style={{
        padding: "8px 12px", borderTop: "1px solid rgba(245,166,35,0.1)",
        display: "flex", gap: 8, background: "rgba(0,0,0,0.2)",
      }}>
        <input
          value={input}
          onChange={e => setInput(e.target.value)}
          onKeyDown={e => e.key === "Enter" && setInput("")}
          placeholder="send chunk..."
          style={{
            flex: 1, background: "rgba(245,166,35,0.05)", border: "1px solid rgba(245,166,35,0.15)",
            borderRadius: 6, padding: "8px 10px", color: "#f0ead6",
            fontFamily: "'Space Mono', monospace", fontSize: 12, outline: "none",
          }}
        />
        <button onClick={() => setInput("")} style={{
          background: "rgba(245,166,35,0.15)", border: "1px solid rgba(245,166,35,0.3)",
          borderRadius: 6, padding: "8px 12px", color: "#f5a623",
          fontFamily: "'Space Mono', monospace", fontSize: 11, cursor: "pointer",
        }}>→</button>
      </div>
    </div>
  );
}

function MessageBubble({ msg }) {
  return (
    <div style={{ display: "flex", flexDirection: "column", alignItems: msg.mine ? "flex-end" : "flex-start", gap: 2 }}>
      {!msg.mine && (
        <span style={{
          fontFamily: "'Space Mono', monospace", fontSize: 9,
          color: "rgba(245,166,35,0.5)", marginLeft: 2,
        }}>{msg.sender} · {msg.senderKey}</span>
      )}
      <div style={{
        maxWidth: "78%", padding: "7px 10px", borderRadius: msg.mine ? "8px 8px 2px 8px" : "8px 8px 8px 2px",
        background: msg.mine ? "rgba(245,166,35,0.15)" : "rgba(255,255,255,0.06)",
        border: `1px solid ${msg.mine ? "rgba(245,166,35,0.25)" : "rgba(255,255,255,0.08)"}`,
      }}>
        <span style={{ fontFamily: "'Space Mono', monospace", fontSize: 12, color: msg.mine ? "#f5c867" : "#d4cdb8", lineHeight: 1.5 }}>
          {msg.text}
        </span>
      </div>
      <div style={{ display: "flex", alignItems: "center", gap: 4, marginLeft: msg.mine ? 0 : 2, marginRight: msg.mine ? 2 : 0 }}>
        <span style={{ fontFamily: "'Space Mono', monospace", fontSize: 8, color: "rgba(255,255,255,0.2)" }}>{msg.ts}</span>
        {msg.encrypted && <LockIcon size={7} />}
      </div>
    </div>
  );
}

// ─── Public Screen ────────────────────────────────────────────────────────────

function PublicScreen() {
  const topicColors = { general: "#5b8dd9", music: "#9b7fd4", alert: "#e05c5c" };
  const typeIcons = { post: "◈", stream_announce: "◎", alert: "⚠" };

  return (
    <div style={{ flex: 1, overflow: "auto", padding: "16px 12px" }}>
      <SectionHeader label="Public Broadcast" count={PUBLIC_POSTS.length} accent="#5b8dd9" />
      <div style={{
        marginBottom: 12, padding: "8px 10px", borderRadius: 6,
        background: "rgba(91,141,217,0.06)", border: "1px solid rgba(91,141,217,0.15)",
        fontFamily: "'Space Mono', monospace", fontSize: 9, color: "rgba(91,141,217,0.7)",
        letterSpacing: "0.05em",
      }}>
        ◎ unencrypted · signed by sender key · anyone in range receives
      </div>

      <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
        {PUBLIC_POSTS.map(post => (
          <div key={post.id} style={{
            padding: "10px 12px", borderRadius: 8,
            background: "rgba(255,255,255,0.02)", border: "1px solid rgba(255,255,255,0.07)",
            display: "flex", flexDirection: "column", gap: 6,
          }}>
            <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
              <span style={{ fontSize: 12, color: topicColors[post.topic] || "#888" }}>{typeIcons[post.type]}</span>
              <span style={{ fontFamily: "'Space Mono', monospace", fontSize: 10, color: "#f0ead6", fontWeight: 700 }}>{post.sender}</span>
              <span style={{ fontFamily: "'Space Mono', monospace", fontSize: 9, color: "rgba(255,255,255,0.2)" }}>{post.senderKey}</span>
              <div style={{ flex: 1 }} />
              <span style={{
                fontSize: 8, fontFamily: "'Space Mono', monospace",
                color: topicColors[post.topic] || "#888",
                background: `${topicColors[post.topic]}18` || "rgba(255,255,255,0.05)",
                padding: "1px 6px", borderRadius: 3,
              }}>{post.topic}</span>
            </div>
            <span style={{ fontFamily: "'Space Mono', monospace", fontSize: 12, color: "#c8c0aa", lineHeight: 1.6 }}>
              {post.text}
            </span>
            <span style={{ fontFamily: "'Space Mono', monospace", fontSize: 8, color: "rgba(255,255,255,0.2)" }}>{post.ts}</span>
          </div>
        ))}
      </div>
    </div>
  );
}

// ─── Profile Screen ───────────────────────────────────────────────────────────

function ProfileScreen() {
  const [name, setName] = useState("my-laptop");
  return (
    <div style={{ flex: 1, overflow: "auto", padding: "16px 12px" }}>
      <SectionHeader label="Identity" count={null} accent="#f5a623" />

      <div style={{ marginBottom: 20, padding: "12px", borderRadius: 8, background: "rgba(245,166,35,0.04)", border: "1px solid rgba(245,166,35,0.15)" }}>
        <div style={{ fontFamily: "'Space Mono', monospace", fontSize: 9, color: "rgba(245,166,35,0.5)", marginBottom: 8, letterSpacing: "0.1em", textTransform: "uppercase" }}>Display Name</div>
        <input
          value={name}
          onChange={e => setName(e.target.value)}
          style={{
            width: "100%", background: "rgba(0,0,0,0.3)", border: "1px solid rgba(245,166,35,0.2)",
            borderRadius: 5, padding: "7px 10px", color: "#f0ead6",
            fontFamily: "'Space Mono', monospace", fontSize: 13, outline: "none", boxSizing: "border-box",
          }}
        />
        <div style={{ fontFamily: "'Space Mono', monospace", fontSize: 9, color: "rgba(255,255,255,0.2)", marginTop: 6 }}>
          name is display-only · does not affect cryptographic identity
        </div>
      </div>

      <div style={{ marginBottom: 20, padding: "12px", borderRadius: 8, background: "rgba(255,255,255,0.02)", border: "1px solid rgba(255,255,255,0.07)" }}>
        <div style={{ fontFamily: "'Space Mono', monospace", fontSize: 9, color: "rgba(255,255,255,0.3)", marginBottom: 8, letterSpacing: "0.1em", textTransform: "uppercase" }}>Device Fingerprint</div>
        <div style={{ fontFamily: "'Space Mono', monospace", fontSize: 16, color: "#f5a623", letterSpacing: "0.15em" }}>
          a4f2·e891
        </div>
        <div style={{ fontFamily: "'Space Mono', monospace", fontSize: 9, color: "rgba(255,255,255,0.2)", marginTop: 6 }}>
          first 8 chars of your public key hash · share to verify identity
        </div>
      </div>

      <div style={{ padding: "12px", borderRadius: 8, background: "rgba(255,255,255,0.02)", border: "1px solid rgba(255,255,255,0.07)" }}>
        <div style={{ fontFamily: "'Space Mono', monospace", fontSize: 9, color: "rgba(255,255,255,0.3)", marginBottom: 10, letterSpacing: "0.1em", textTransform: "uppercase" }}>Known Devices ({PEERS.filter(p => p.trusted).length})</div>
        {PEERS.filter(p => p.trusted).map(peer => (
          <div key={peer.id} style={{ display: "flex", alignItems: "center", gap: 8, marginBottom: 8 }}>
            <PulseDot color="#4a9e6b" size={6} />
            <span style={{ fontFamily: "'Space Mono', monospace", fontSize: 11, color: "#f0ead6", flex: 1 }}>{peer.name}</span>
            <span style={{ fontFamily: "'Space Mono', monospace", fontSize: 9, color: "rgba(255,255,255,0.25)" }}>{peer.id}</span>
          </div>
        ))}
      </div>
    </div>
  );
}

// ─── App Root ─────────────────────────────────────────────────────────────────

export default function App() {
  const [tab, setTab] = useState("nearby");
  const [conversation, setConversation] = useState(null);

  const handleOpenDM = (peer) => {
    setConversation({ type: "dm", target: peer, messages: DM_MESSAGES });
    setTab("channels");
  };

  const handleOpenChannel = (channel) => {
    setConversation({ type: "channel", target: channel, messages: CHANNEL_MESSAGES[channel.id] || [] });
  };

  const handleBack = () => setConversation(null);

  const renderMain = () => {
    if (conversation) {
      return <ConversationView target={conversation.target} messages={conversation.messages} onBack={handleBack} />;
    }
    switch (tab) {
      case "nearby": return <NearbyScreen onOpenDM={handleOpenDM} />;
      case "channels": return <ChannelsScreen onOpenChannel={handleOpenChannel} />;
      case "public": return <PublicScreen />;
      case "profile": return <ProfileScreen />;
      default: return <NearbyScreen onOpenDM={handleOpenDM} />;
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
              SUMMIT
            </div>
            <div style={{ fontFamily: "'Space Mono', monospace", fontSize: 8, color: "rgba(255,255,255,0.25)", letterSpacing: "0.15em", marginTop: 1 }}>
              NO CLOUD · NO INFRA · PROXIMAL
            </div>
          </div>
          <div style={{ display: "flex", alignItems: "center", gap: 6 }}>
            <PulseDot color="#4a9e6b" size={7} />
            <span style={{ fontFamily: "'Space Mono', monospace", fontSize: 9, color: "#4a9e6b" }}>summitd running</span>
          </div>
        </div>

        {/* Main content */}
        <div style={{ flex: 1, display: "flex", flexDirection: "column", overflow: "hidden" }}>
          {renderMain()}
        </div>

        {/* Nav */}
        <div style={{ position: "relative" }}>
          <NavBar active={tab} onNav={(t) => { setTab(t); setConversation(null); }} />
        </div>
      </div>
    </>
  );
}
