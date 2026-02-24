import Badge from '../common/Badge';
import { shortKey, timeAgo } from '../../lib/format';

function statusVariant(status) {
  const s = (status || '').toLowerCase();
  if (s === 'completed' || s === 'done') return 'completed';
  if (s === 'running' || s === 'in_progress') return 'running';
  if (s === 'failed' || s === 'error') return 'failed';
  if (s === 'queued' || s === 'pending') return 'queued';
  if (s === 'cancelled') return 'failed';
  return 'pending';
}

export default function TaskCard({ task, onClick }) {
  return (
    <button
      onClick={() => onClick?.(task)}
      className="w-full text-left bg-summit-raised/40 border border-summit-border rounded-lg p-3.5 flex flex-col gap-2 hover:border-summit-border-bright hover:bg-summit-raised/60 transition-all duration-150 cursor-pointer group"
    >
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-2.5">
          <span className="text-[12px] font-bold text-summit-cream group-hover:text-summit-white transition-colors">
            {shortKey(task.id || task.task_id || 'unknown')}
          </span>
          <Badge variant={statusVariant(task.status)} />
        </div>
        {task.peer && (
          <span className="text-[9px] text-white/20 tracking-wide">
            → {shortKey(task.peer)}
          </span>
        )}
      </div>
      {(task.submitted_at || task.created_at) && (
        <span className="text-[9px] text-white/15">
          submitted {timeAgo(Math.floor((Date.now() - (task.submitted_at || task.created_at)) / 1000))}
        </span>
      )}
    </button>
  );
}
