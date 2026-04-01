import type { TourStepProps } from 'antd';

export function getDashboardSteps(t: (k: string) => string): TourStepProps[] {
  return [
    {
      title: t('tour.dashboard.step1.title'),
      description: t('tour.dashboard.step1.desc'),
      target: () => document.querySelector('[data-tour="sidebar"]') as HTMLElement,
      placement: 'right',
    },
    {
      title: t('tour.dashboard.step2.title'),
      description: t('tour.dashboard.step2.desc'),
      target: () => document.querySelector('[data-tour="stats-row"]') as HTMLElement,
      placement: 'bottom',
    },
    {
      title: t('tour.dashboard.step3.title'),
      description: t('tour.dashboard.step3.desc'),
      target: () => document.querySelector('[data-tour="health-card"]') as HTMLElement,
      placement: 'bottom',
    },
    {
      title: t('tour.dashboard.step4.title'),
      description: t('tour.dashboard.step4.desc'),
      target: () => document.querySelector('[data-tour="guides-button"]') as HTMLElement,
      placement: 'bottom',
    },
    {
      title: t('tour.dashboard.step5.title'),
      description: t('tour.dashboard.step5.desc'),
      target: null as unknown as undefined,
      placement: 'center',
    },
  ];
}
