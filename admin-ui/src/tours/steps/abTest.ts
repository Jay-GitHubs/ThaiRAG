import type { TourStepProps } from 'antd';

export function getAbTestSteps(t: (k: string) => string): TourStepProps[] {
  return [
    {
      title: t('tour.abTests.step1.title'),
      description: t('tour.abTests.step1.desc'),
      target: () => document.querySelector('[data-tour="ab-create"]') as HTMLElement,
      placement: 'bottom',
    },
    {
      title: t('tour.abTests.step2.title'),
      description: t('tour.abTests.step2.desc'),
      target: () => document.querySelector('[data-tour="ab-table"]') as HTMLElement,
      placement: 'top',
    },
    {
      title: t('tour.abTests.step3.title'),
      description: t('tour.abTests.step3.desc'),
      target: null as unknown as undefined,
      placement: 'center',
    },
  ];
}
