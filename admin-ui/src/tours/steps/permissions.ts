import type { TourStepProps } from 'antd';

export function getPermissionsSteps(t: (k: string) => string): TourStepProps[] {
  return [
    {
      title: t('tour.permissions.step1.title'),
      description: t('tour.permissions.step1.desc'),
      target: () => document.querySelector('[data-tour="perm-scope"]') as HTMLElement,
      placement: 'bottom',
    },
    {
      title: t('tour.permissions.step2.title'),
      description: t('tour.permissions.step2.desc'),
      target: () => document.querySelector('[data-tour="perm-table"]') as HTMLElement,
      placement: 'top',
    },
    {
      title: t('tour.permissions.step3.title'),
      description: t('tour.permissions.step3.desc'),
      target: null as unknown as undefined,
      placement: 'center',
    },
  ];
}
