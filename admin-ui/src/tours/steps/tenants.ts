import type { TourStepProps } from 'antd';

export function getTenantsSteps(t: (k: string) => string): TourStepProps[] {
  return [
    {
      title: t('tour.tenants.step1.title'),
      description: t('tour.tenants.step1.desc'),
      target: () => document.querySelector('[data-tour="tenants-create"]') as HTMLElement,
      placement: 'bottom',
    },
    {
      title: t('tour.tenants.step2.title'),
      description: t('tour.tenants.step2.desc'),
      target: () => document.querySelector('[data-tour="tenants-table"]') as HTMLElement,
      placement: 'top',
    },
    {
      title: t('tour.tenants.step3.title'),
      description: t('tour.tenants.step3.desc'),
      target: () => document.querySelector('[data-tour="tenants-detail"]') as HTMLElement,
      placement: 'left',
    },
    {
      title: t('tour.tenants.step4.title'),
      description: t('tour.tenants.step4.desc'),
      target: null as unknown as undefined,
      placement: 'center',
    },
  ];
}
