import { useMemo } from 'react';
import { useDaemon } from '../../hooks/useDaemon';
import SectionHeader from '../common/SectionHeader';
import TaskCard from './TaskCard';

export default function TaskList({ onSelectTask }) {
  const { computeTasks } = useDaemon();

  const { active, completed } = useMemo(() => {
    const tasks = [...(computeTasks || [])];
    const st = (t) => (t.status || '').toLowerCase();
    tasks.sort((a, b) => (b.submitted_at || 0) - (a.submitted_at || 0));
    return {
      active: tasks.filter(t => ['running', 'in_progress', 'queued', 'pending'].includes(st(t))),
      completed: tasks.filter(t => ['completed', 'done', 'failed', 'error', 'cancelled'].includes(st(t))),
    };
  }, [computeTasks]);

  return (
    <div className="flex flex-col gap-5">
      <div>
        <SectionHeader label="Active Tasks" count={active.length} />
        {active.length === 0 ? (
          <div className="bg-summit-raised/30 border border-summit-border rounded-xl py-8 text-center">
            <div className="text-[11px] text-white/20">No active tasks</div>
            <div className="text-[9px] text-white/10 mt-1">Submit a task above to get started</div>
          </div>
        ) : (
          <div className="flex flex-col gap-2">
            {active.map(task => (
              <TaskCard key={task.task_id} task={task} onClick={onSelectTask} />
            ))}
          </div>
        )}
      </div>

      {completed.length > 0 && (
        <div>
          <SectionHeader label="History" count={completed.length} />
          <div className="flex flex-col gap-2">
            {completed.map(task => (
              <TaskCard key={task.task_id} task={task} onClick={onSelectTask} />
            ))}
          </div>
        </div>
      )}
    </div>
  );
}
