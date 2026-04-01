import { createContext, useState, useCallback, type ReactNode } from 'react';
import * as storage from './tourStorage';

interface TourContextValue {
  activeTourId: string | null;
  startTour: (tourId: string) => void;
  endTour: () => void;
  completeTour: (tourId: string) => void;
  isTourCompleted: (tourId: string) => boolean;
  resetTour: (tourId: string) => void;
  resetAllTours: () => void;
  isGuidesOpen: boolean;
  setGuidesOpen: (open: boolean) => void;
}

export const TourContext = createContext<TourContextValue>({
  activeTourId: null,
  startTour: () => {},
  endTour: () => {},
  completeTour: () => {},
  isTourCompleted: () => false,
  resetTour: () => {},
  resetAllTours: () => {},
  isGuidesOpen: false,
  setGuidesOpen: () => {},
});

export function TourProvider({ children }: { children: ReactNode }) {
  const [activeTourId, setActiveTourId] = useState<string | null>(null);
  const [isGuidesOpen, setGuidesOpen] = useState(false);
  // Force re-render when tour state changes
  const [, setVersion] = useState(0);

  const startTour = useCallback((tourId: string) => {
    setGuidesOpen(false);
    setActiveTourId(tourId);
  }, []);

  const endTour = useCallback(() => {
    setActiveTourId(null);
  }, []);

  const completeTour = useCallback((tourId: string) => {
    storage.setTourCompleted(tourId);
    setActiveTourId(null);
    setVersion((v) => v + 1);
  }, []);

  const isTourCompleted = useCallback((tourId: string) => {
    return storage.isTourCompleted(tourId);
  }, []);

  const resetTour = useCallback((tourId: string) => {
    storage.resetTour(tourId);
    setVersion((v) => v + 1);
  }, []);

  const resetAllTours = useCallback(() => {
    storage.resetAllTours();
    setVersion((v) => v + 1);
  }, []);

  return (
    <TourContext.Provider
      value={{
        activeTourId,
        startTour,
        endTour,
        completeTour,
        isTourCompleted,
        resetTour,
        resetAllTours,
        isGuidesOpen,
        setGuidesOpen,
      }}
    >
      {children}
    </TourContext.Provider>
  );
}
