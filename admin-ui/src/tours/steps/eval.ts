import type { TourStepProps } from 'antd';

export function getEvalSteps(t: (k: string) => string): TourStepProps[] {
  return [
    {
      title: t('tour.eval.step1.title'),
      description: t('tour.eval.step1.desc'),
      target: () => document.querySelector('[data-tour="eval-actions"]') as HTMLElement,
      placement: 'bottom',
    },
    {
      title: t('tour.eval.step2.title'),
      description: t('tour.eval.step2.desc'),
      target: () => document.querySelector('[data-tour="eval-table"]') as HTMLElement,
      placement: 'top',
    },
    {
      title: t('tour.eval.step3.title'),
      description: t('tour.eval.step3.desc'),
      target: () => document.querySelector('[data-tour="eval-results"]') as HTMLElement,
      placement: 'top',
    },
  ];
}
