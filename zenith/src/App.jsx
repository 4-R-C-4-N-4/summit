import { DaemonProvider } from './context/DaemonContext';
import { UIProvider } from './context/UIContext';
import AppShell from './components/layout/AppShell';
import ErrorBoundary from './components/common/ErrorBoundary';

export default function App() {
  return (
    <ErrorBoundary>
      <DaemonProvider>
        <UIProvider>
          <AppShell />
        </UIProvider>
      </DaemonProvider>
    </ErrorBoundary>
  );
}
