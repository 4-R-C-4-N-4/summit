import { useDaemon } from '../../hooks/useDaemon';
import { bytesFormat } from '../../lib/format';

export default function StatusBar() {
  const { connected, computeTasks, cache } = useDaemon();
  const taskCount = computeTasks?.length || 0;

  return (
    <footer className="h-6 px-4 flex items-center justify-between border-t border-summit-border bg-summit-surface/60 text-[9px] text-white/25 tracking-wide select-none">
      <div className="flex items-center gap-3">
        <span className="flex items-center gap-1">
          <span className={`inline-block w-1.5 h-1.5 rounded-full ${connected ? 'bg-summit-green/60' : 'bg-summit-red/60'}`} />
          {connected ? 'connected' : 'offline'}
        </span>
        <span className="text-white/10">|</span>
        <span>{taskCount} compute task{taskCount !== 1 ? 's' : ''}</span>
      </div>
      <div className="flex items-center gap-3">
        <span>cache {cache?.chunks || 0} chunks · {bytesFormat(cache?.bytes || 0)}</span>
      </div>
    </footer>
  );
}
