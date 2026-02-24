import { useUIState } from '../../hooks/useUIState';
import Sidebar from './Sidebar';
import HeaderBar from './HeaderBar';
import StatusBar from './StatusBar';
import ContextPanel from './ContextPanel';
import DashboardView from '../dashboard/DashboardView';
import ComputeView from '../compute/ComputeView';
import FilesView from '../files/FilesView';
import MessagesView from '../messages/MessagesView';
import SystemView from '../system/SystemView';

const VIEWS = {
  home: DashboardView,
  compute: ComputeView,
  files: FilesView,
  messages: MessagesView,
  system: SystemView,
};

export default function AppShell() {
  const { activeView } = useUIState();
  const ViewComponent = VIEWS[activeView] || DashboardView;

  return (
    <div className="h-screen w-screen flex bg-summit-bg overflow-hidden">
      <Sidebar />
      <div className="flex-1 flex flex-col min-w-0">
        <HeaderBar />
        <main className="flex-1 overflow-auto">
          <ViewComponent />
        </main>
        <StatusBar />
      </div>
      <ContextPanel />
    </div>
  );
}
