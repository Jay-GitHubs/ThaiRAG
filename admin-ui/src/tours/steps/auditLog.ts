import type { TourStepProps } from 'antd';

export function getAuditLogSteps(t: (k: string) => string): TourStepProps[] {
  return [
    {
      title: t('tour.auditLog.step1.title'),
      description: t('tour.auditLog.step1.desc'),
      target: () => document.querySelector('[data-tour="audit-filters"]') as HTMLElement,
      placement: 'bottom',
    },
    {
      title: t('tour.auditLog.step2.title'),
      description: t('tour.auditLog.step2.desc'),
      target: () => document.querySelector('[data-tour="audit-table"]') as HTMLElement,
      placement: 'top',
    },
    {
      title: t('tour.auditLog.step3.title'),
      description: t('tour.auditLog.step3.desc'),
      target: () => document.querySelector('[data-tour="audit-export"]') as HTMLElement,
      placement: 'bottom',
    },
  ];
}
