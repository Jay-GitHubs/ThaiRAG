import { useContext } from 'react';
import { TourContext } from './TourContext';

export function useTour(tourId: string) {
  const ctx = useContext(TourContext);
  return {
    isActive: ctx.activeTourId === tourId,
    start: () => ctx.startTour(tourId),
    end: () => ctx.endTour(),
    complete: () => ctx.completeTour(tourId),
  };
}
