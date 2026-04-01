import type { TourStepProps } from 'antd';

export function getAnalyticsSteps(t: (k: string) => string): TourStepProps[] {
  return [
    {
      title: t('tour.analytics.step1.title'),
      description: t('tour.analytics.step1.desc'),
      target: () => document.querySelector('[data-tour="analytics-stats"]') as HTMLElement,
      placement: 'bottom',
    },
    {
      title: t('tour.analytics.step2.title'),
      description: t('tour.analytics.step2.desc'),
      target: () => document.querySelector('[data-tour="analytics-time"]') as HTMLElement,
      placement: 'bottom',
    },
    {
      title: t('tour.analytics.step3.title'),
      description: t('tour.analytics.step3.desc'),
      target: () => document.querySelector('[data-tour="analytics-charts"]') as HTMLElement,
      placement: 'top',
    },
  ];
}
