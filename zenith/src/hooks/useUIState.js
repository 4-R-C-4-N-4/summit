import { useContext } from 'react';
import { UIContext } from '../context/UIContext';

export function useUIState() {
  return useContext(UIContext);
}
