import { useState } from 'react';
import TaskSubmitForm from './TaskSubmitForm';
import TaskList from './TaskList';
import TaskDetail from './TaskDetail';

export default function ComputeView() {
  const [selectedTask, setSelectedTask] = useState(null);

  return (
    <div className="h-full flex" style={{ animation: 'fadeIn 0.25s ease-out' }}>
      <div className="flex-1 p-6 overflow-auto">
        <div className="max-w-4xl mx-auto flex flex-col gap-5">
          <div>
            <h2 className="text-xl font-bold text-summit-white tracking-[0.08em] uppercase mb-1">
              Compute
            </h2>
            <p className="text-[10px] text-white/25 tracking-wide">
              Submit tasks to trusted peers on the mesh
            </p>
          </div>

          <TaskSubmitForm />
          <TaskList onSelectTask={setSelectedTask} />
        </div>
      </div>

      {selectedTask && (
        <div className="w-[380px] border-l border-summit-border bg-summit-surface overflow-y-auto" style={{ animation: 'slideIn 0.2s ease-out' }}>
          <div className="flex items-center justify-between px-4 py-3 border-b border-summit-border">
            <span className="text-[10px] font-bold text-summit-accent tracking-[0.1em] uppercase">
              Task Detail
            </span>
            <button
              onClick={() => setSelectedTask(null)}
              className="w-6 h-6 flex items-center justify-center rounded text-white/25 hover:text-white/50 hover:bg-white/5 transition-colors cursor-pointer"
            >
              <svg width="12" height="12" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round">
                <path d="M4 4l8 8M12 4l-8 8" />
              </svg>
            </button>
          </div>
          <TaskDetail task={selectedTask} />
        </div>
      )}
    </div>
  );
}
