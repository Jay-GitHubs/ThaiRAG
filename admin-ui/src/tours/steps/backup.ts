import type { TourStepProps } from 'antd';

export function getBackupSteps(t: (k: string) => string): TourStepProps[] {
  return [
    {
      title: t('tour.backup.step1.title'),
      description: t('tour.backup.step1.desc'),
      target: () => document.querySelector('[data-tour="backup-create"]') as HTMLElement,
      placement: 'bottom',
    },
    {
      title: t('tour.backup.step2.title'),
      description: t('tour.backup.step2.desc'),
      target: () => document.querySelector('[data-tour="backup-list"]') as HTMLElement,
      placement: 'top',
    },
    {
      title: t('tour.backup.step3.title'),
      description: t('tour.backup.step3.desc'),
      target: () => document.querySelector('[data-tour="backup-restore"]') as HTMLElement,
      placement: 'top',
    },
  ];
}
