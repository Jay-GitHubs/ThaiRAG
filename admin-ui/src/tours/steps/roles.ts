import type { TourStepProps } from 'antd';

export function getRolesSteps(t: (k: string) => string): TourStepProps[] {
  return [
    {
      title: t('tour.roles.step1.title'),
      description: t('tour.roles.step1.desc'),
      target: () => document.querySelector('[data-tour="roles-create"]') as HTMLElement,
      placement: 'bottom',
    },
    {
      title: t('tour.roles.step2.title'),
      description: t('tour.roles.step2.desc'),
      target: () => document.querySelector('[data-tour="roles-table"]') as HTMLElement,
      placement: 'top',
    },
    {
      title: t('tour.roles.step3.title'),
      description: t('tour.roles.step3.desc'),
      target: null as unknown as undefined,
      placement: 'center',
    },
  ];
}
