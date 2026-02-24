import { useContext } from 'react';
import { DaemonContext } from '../context/DaemonContext';

export function useDaemon() {
  return useContext(DaemonContext);
}
