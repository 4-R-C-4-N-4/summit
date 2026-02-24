import { useDaemon } from '../../hooks/useDaemon';
import SessionList from './SessionList';
import CachePanel from './CachePanel';
import ServicesList from './ServicesList';
import SettingsPanel from './SettingsPanel';

export default function SystemView() {
  const { status } = useDaemon();

  return (
    <div className="p-6 overflow-auto h-full" style={{ animation: 'fadeIn 0.25s ease-out' }}>
      <div className="max-w-3xl mx-auto flex flex-col gap-5">
        <div>
          <h2 className="text-xl font-bold text-summit-white tracking-[0.08em] uppercase mb-1">
            System
          </h2>
          <p className="text-[10px] text-white/25 tracking-wide">
            Daemon health and configuration
          </p>
        </div>

        <div className="bg-summit-raised/60 border border-summit-border rounded-xl p-5 grid grid-cols-2 gap-6">
          <div>
            <div className="text-[10px] text-white/30 tracking-wide mb-1">Peers Discovered</div>
            <div className="text-2xl font-bold text-summit-white tabular-nums">{status?.peers_discovered || 0}</div>
          </div>
          <div>
            <div className="text-[10px] text-white/30 tracking-wide mb-1">Active Sessions</div>
            <div className="text-2xl font-bold text-summit-white tabular-nums">{status?.sessions?.length || 0}</div>
          </div>
        </div>

        <SessionList />
        <CachePanel />
        <ServicesList />
        <SettingsPanel />
      </div>
    </div>
  );
}
