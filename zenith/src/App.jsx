import { DaemonProvider } from './context/DaemonContext';
import { UIProvider } from './context/UIContext';
import AppShell from './components/layout/AppShell';

export default function App() {
  return (
    <DaemonProvider>
      <UIProvider>
        <AppShell />
      </UIProvider>
    </DaemonProvider>
  );
}
