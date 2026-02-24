import Badge from '../common/Badge';
import CopyButton from '../common/CopyButton';
import { shortKey, timeAgo } from '../../lib/format';

function statusVariant(status) {
  const s = (status || '').toLowerCase();
  if (s === 'completed' || s === 'done') return 'completed';
  if (s === 'failed' || s === 'error') return 'failed';
  if (s === 'queued' || s === 'pending') return 'queued';
  if (s === 'cancelled') return 'failed';
  return 'running';
}

export default function TaskDetail({ task }) {
  if (!task) return null;

  const hasError = task.result?.error;
  const resultStr = task.result ? JSON.stringify(task.result, null, 2) : null;

  return (
    <div className="p-4 flex flex-col gap-4">
      <div className="flex items-center justify-between">
        <span className="text-[14px] font-bold text-summit-white">
          Task {shortKey(task.task_id || task.id || '')}
        </span>
        <Badge variant={statusVariant(task.status)} />
      </div>

      {/* Timing info */}
      <div className="bg-summit-raised/50 border border-summit-border rounded-lg p-3 grid grid-cols-2 gap-3">
        {task.submitted_at && (
          <div>
            <div className="text-[9px] text-white/25 tracking-wider uppercase mb-0.5">Submitted</div>
            <div className="text-[10px] text-summit-cream">
              {timeAgo(Math.floor((Date.now() - task.submitted_at) / 1000))}
            </div>
          </div>
        )}
        {task.elapsed_ms != null && (
          <div>
            <div className="text-[9px] text-white/25 tracking-wider uppercase mb-0.5">Duration</div>
            <div className="text-[10px] text-summit-cream">{task.elapsed_ms}ms</div>
          </div>
        )}
      </div>

      {task.peer && (
        <div>
          <div className="text-[9px] text-white/25 tracking-wider uppercase mb-1">Peer</div>
          <div className="flex items-center gap-2">
            <span className="text-[10px] text-summit-cream truncate">{task.peer}</span>
            <CopyButton text={task.peer} />
          </div>
        </div>
      )}

      {task.payload && (
        <div className="flex flex-col gap-1.5">
          <span className="text-[9px] text-white/25 tracking-wider uppercase">Payload</span>
          <pre className="bg-summit-bg border border-summit-border rounded-lg p-3 text-[10px] text-white/50 overflow-auto max-h-40">
            {JSON.stringify(task.payload, null, 2)}
          </pre>
        </div>
      )}

      {resultStr && (
        <div className="flex flex-col gap-1.5">
          <div className="flex items-center gap-2">
            <span className="text-[9px] text-white/25 tracking-wider uppercase">Result</span>
            <CopyButton text={resultStr} />
          </div>
          <pre className={`bg-summit-bg border rounded-lg p-3 text-[10px] overflow-auto max-h-60 ${hasError ? 'border-summit-red/15 text-summit-red/70' : 'border-summit-green/15 text-summit-green/70'}`}>
            {resultStr}
          </pre>
        </div>
      )}
    </div>
  );
}
