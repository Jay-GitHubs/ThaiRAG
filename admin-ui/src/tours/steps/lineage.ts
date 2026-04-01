import type { TourStepProps } from 'antd';

export function getLineageSteps(t: (k: string) => string): TourStepProps[] {
  return [
    {
      title: t('tour.lineage.step1.title'),
      description: t('tour.lineage.step1.desc'),
      target: () => document.querySelector('[data-tour="lineage-tabs"]') as HTMLElement,
      placement: 'bottom',
    },
    {
      title: t('tour.lineage.step2.title'),
      description: t('tour.lineage.step2.desc'),
      target: () => document.querySelector('[data-tour="lineage-input"]') as HTMLElement,
      placement: 'bottom',
    },
    {
      title: t('tour.lineage.step3.title'),
      description: t('tour.lineage.step3.desc'),
      target: () => document.querySelector('[data-tour="lineage-results"]') as HTMLElement,
      placement: 'top',
    },
  ];
}
