const STORAGE_KEY = 'thairag-tour-state';

export function getTourState(): Record<string, boolean> {
  try {
    return JSON.parse(localStorage.getItem(STORAGE_KEY) || '{}');
  } catch {
    return {};
  }
}

export function isTourCompleted(tourId: string): boolean {
  return getTourState()[tourId] === true;
}

export function setTourCompleted(tourId: string): void {
  const state = getTourState();
  state[tourId] = true;
  localStorage.setItem(STORAGE_KEY, JSON.stringify(state));
}

export function resetTour(tourId: string): void {
  const state = getTourState();
  delete state[tourId];
  localStorage.setItem(STORAGE_KEY, JSON.stringify(state));
}

export function resetAllTours(): void {
  localStorage.removeItem(STORAGE_KEY);
}

export function isFirstVisit(): boolean {
  return localStorage.getItem(STORAGE_KEY) === null;
}
