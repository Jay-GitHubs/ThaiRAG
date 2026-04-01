import type { TourStepProps } from 'antd';

export function getConnectorsSteps(t: (k: string) => string): TourStepProps[] {
  return [
    {
      title: t('tour.connectors.step1.title'),
      description: t('tour.connectors.step1.desc'),
      target: () => document.querySelector('[data-tour="connectors-add"]') as HTMLElement,
      placement: 'bottom',
    },
    {
      title: t('tour.connectors.step2.title'),
      description: t('tour.connectors.step2.desc'),
      target: () => document.querySelector('[data-tour="connectors-list"]') as HTMLElement,
      placement: 'top',
    },
    {
      title: t('tour.connectors.step3.title'),
      description: t('tour.connectors.step3.desc'),
      target: null as unknown as undefined,
      placement: 'center',
    },
  ];
}
