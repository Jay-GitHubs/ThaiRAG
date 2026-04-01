import type { TourStepProps } from 'antd';

export function getSearchAnalyticsSteps(t: (k: string) => string): TourStepProps[] {
  return [
    {
      title: t('tour.searchAnalytics.step1.title'),
      description: t('tour.searchAnalytics.step1.desc'),
      target: () => document.querySelector('[data-tour="sa-stats"]') as HTMLElement,
      placement: 'bottom',
    },
    {
      title: t('tour.searchAnalytics.step2.title'),
      description: t('tour.searchAnalytics.step2.desc'),
      target: () => document.querySelector('[data-tour="sa-popular"]') as HTMLElement,
      placement: 'top',
    },
    {
      title: t('tour.searchAnalytics.step3.title'),
      description: t('tour.searchAnalytics.step3.desc'),
      target: () => document.querySelector('[data-tour="sa-zero"]') as HTMLElement,
      placement: 'top',
    },
  ];
}
