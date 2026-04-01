import type { TourStepProps } from 'antd';

export function getUsageSteps(t: (k: string) => string): TourStepProps[] {
  return [
    {
      title: t('tour.usage.step1.title'),
      description: t('tour.usage.step1.desc'),
      target: () => document.querySelector('[data-tour="usage-cost"]') as HTMLElement,
      placement: 'bottom',
    },
    {
      title: t('tour.usage.step2.title'),
      description: t('tour.usage.step2.desc'),
      target: () => document.querySelector('[data-tour="usage-tokens"]') as HTMLElement,
      placement: 'bottom',
    },
    {
      title: t('tour.usage.step3.title'),
      description: t('tour.usage.step3.desc'),
      target: () => document.querySelector('[data-tour="usage-provider"]') as HTMLElement,
      placement: 'top',
    },
  ];
}
