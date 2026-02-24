import NetworkWidget from './NetworkWidget';
import ComputeSummary from './ComputeSummary';
import TransferSummary from './TransferSummary';
import MessagesSummary from './MessagesSummary';
import { useDaemon } from '../../hooks/useDaemon';
import { shortKey } from '../../lib/format';
import CopyButton from '../common/CopyButton';

export default function DashboardView() {
  const { status, connected } = useDaemon();

  return (
    <div className="p-6 overflow-auto h-full" style={{ animation: 'fadeIn 0.25s ease-out' }}>
      <div className="max-w-6xl mx-auto flex flex-col gap-5">
        {/* Header area */}
        <div className="flex items-end justify-between">
          <div>
            <h2 className="text-xl font-bold text-summit-white tracking-[0.08em] uppercase mb-1">
              Dashboard
            </h2>
            {status?.public_key ? (
              <div className="flex items-center gap-2">
                <span className="text-[10px] text-white/30 tracking-wide">
                  node {shortKey(status.public_key)}
                </span>
                <CopyButton text={status.public_key} />
              </div>
            ) : (
              <span className="text-[10px] text-white/20 tracking-wide">
                {connected ? 'Initializing...' : 'Daemon not connected'}
              </span>
            )}
          </div>
        </div>

        {/* Network topology — hero */}
        <NetworkWidget />

        {/* Summary cards grid */}
        <div className="grid grid-cols-1 md:grid-cols-3 gap-3">
          <ComputeSummary />
          <TransferSummary />
          <MessagesSummary />
        </div>
      </div>
    </div>
  );
}
