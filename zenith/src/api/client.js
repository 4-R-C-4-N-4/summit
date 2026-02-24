const API_BASE = 'http://127.0.0.1:9001/api';

async function request(path, options = {}) {
  let res;
  try {
    res = await fetch(`${API_BASE}${path}`, options);
  } catch (err) {
    throw new Error('Daemon unreachable');
  }

  const text = await res.text();

  let data;
  try {
    data = text ? JSON.parse(text) : {};
  } catch {
    throw new Error(text || `HTTP ${res.status}`);
  }

  if (!res.ok) {
    throw new Error(data?.error || data?.message || `HTTP ${res.status}`);
  }

  return data;
}

const get = (path) => request(path);
const post = (path, body) => request(path, {
  method: 'POST',
  headers: { 'Content-Type': 'application/json' },
  body: body !== undefined ? JSON.stringify(body) : undefined,
});
const del = (path) => request(path, { method: 'DELETE' });

export const api = {
  // Status
  getStatus: () => get('/status'),

  // Peers
  getPeers: () => get('/peers'),

  // Trust
  getTrust: () => get('/trust'),
  getTrustPending: () => get('/trust/pending'),
  trustPeer: (publicKey) => post('/trust/add', { public_key: publicKey }),
  blockPeer: (publicKey) => post('/trust/block', { public_key: publicKey }),

  // Sessions
  inspectSession: (sessionId) => get(`/sessions/${sessionId}`),
  dropSession: (sessionId) => del(`/sessions/${sessionId}`),

  // Files
  getFiles: () => get('/files'),
  sendFile: async (file, target) => {
    const formData = new FormData();
    formData.append('file', file);
    if (target) formData.append('target', JSON.stringify(target));
    return request('/send', { method: 'POST', body: formData });
  },

  // Cache
  getCacheStats: () => get('/cache'),
  clearCache: () => post('/cache/clear'),

  // Schemas
  getSchemas: () => get('/schema'),

  // Services
  getServices: () => get('/services'),

  // Messages
  getMessages: (peerPubkey) => get(`/messages/${peerPubkey}`),
  sendMessage: (toPubkey, text) => post('/messages/send', { to: toPubkey, text }),

  // Compute
  getComputeTasks: () => get('/compute/tasks'),
  getComputeTasksForPeer: (peer) => get(`/compute/tasks/${peer}`),
  submitComputeTask: (peer, payload) => post('/compute/submit', { to: peer, payload }),
};
