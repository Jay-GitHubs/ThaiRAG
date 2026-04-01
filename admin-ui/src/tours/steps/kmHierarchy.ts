import type { TourStepProps } from 'antd';

export function getKmHierarchySteps(t: (k: string) => string): TourStepProps[] {
  return [
    {
      title: t('tour.km.step1.title'),
      description: t('tour.km.step1.desc'),
      target: () => document.querySelector('[data-tour="km-tree"]') as HTMLElement,
      placement: 'right',
    },
    {
      title: t('tour.km.step2.title'),
      description: t('tour.km.step2.desc'),
      target: () => document.querySelector('[data-tour="km-add-org"]') as HTMLElement,
      placement: 'bottom',
    },
    {
      title: t('tour.km.step3.title'),
      description: t('tour.km.step3.desc'),
      target: () => document.querySelector('[data-tour="km-detail"]') as HTMLElement,
      placement: 'left',
    },
    {
      title: t('tour.km.step4.title'),
      description: t('tour.km.step4.desc'),
      target: null as unknown as undefined,
      placement: 'center',
    },
  ];
}
