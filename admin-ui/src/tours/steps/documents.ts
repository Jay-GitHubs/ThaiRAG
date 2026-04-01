import type { TourStepProps } from 'antd';

export function getDocumentsSteps(t: (k: string) => string): TourStepProps[] {
  return [
    {
      title: t('tour.documents.step1.title'),
      description: t('tour.documents.step1.desc'),
      target: () => document.querySelector('[data-tour="doc-org-select"]') as HTMLElement,
      placement: 'bottom',
    },
    {
      title: t('tour.documents.step2.title'),
      description: t('tour.documents.step2.desc'),
      target: () => document.querySelector('[data-tour="doc-dept-select"]') as HTMLElement,
      placement: 'bottom',
    },
    {
      title: t('tour.documents.step3.title'),
      description: t('tour.documents.step3.desc'),
      target: () => document.querySelector('[data-tour="doc-ws-select"]') as HTMLElement,
      placement: 'bottom',
    },
    {
      title: t('tour.documents.step4.title'),
      description: t('tour.documents.step4.desc'),
      target: () => document.querySelector('[data-tour="doc-table"]') as HTMLElement,
      placement: 'top',
    },
    {
      title: t('tour.documents.step5.title'),
      description: t('tour.documents.step5.desc'),
      target: null as unknown as undefined,
      placement: 'center',
    },
  ];
}
