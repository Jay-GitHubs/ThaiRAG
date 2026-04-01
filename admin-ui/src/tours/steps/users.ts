import type { TourStepProps } from 'antd';

export function getUsersSteps(t: (k: string) => string): TourStepProps[] {
  return [
    {
      title: t('tour.users.step1.title'),
      description: t('tour.users.step1.desc'),
      target: () => document.querySelector('[data-tour="users-search"]') as HTMLElement,
      placement: 'bottom',
    },
    {
      title: t('tour.users.step2.title'),
      description: t('tour.users.step2.desc'),
      target: () => document.querySelector('[data-tour="users-table"]') as HTMLElement,
      placement: 'top',
    },
    {
      title: t('tour.users.step3.title'),
      description: t('tour.users.step3.desc'),
      target: () => document.querySelector('[data-tour="users-create"]') as HTMLElement,
      placement: 'bottom',
    },
    {
      title: t('tour.users.step4.title'),
      description: t('tour.users.step4.desc'),
      target: null as unknown as undefined,
      placement: 'center',
    },
  ];
}
