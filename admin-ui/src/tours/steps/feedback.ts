import type { TourStepProps } from 'antd';

export function getFeedbackSteps(t: (k: string) => string): TourStepProps[] {
  return [
    {
      title: t('tour.feedback.step1.title'),
      description: t('tour.feedback.step1.desc'),
      target: () => document.querySelector('[data-tour="feedback-tabs"]') as HTMLElement,
      placement: 'bottom',
    },
    {
      title: t('tour.feedback.step2.title'),
      description: t('tour.feedback.step2.desc'),
      target: () => document.querySelector('[data-tour="feedback-stats"]') as HTMLElement,
      placement: 'bottom',
    },
    {
      title: t('tour.feedback.step3.title'),
      description: t('tour.feedback.step3.desc'),
      target: () => document.querySelector('[data-tour="feedback-content"]') as HTMLElement,
      placement: 'top',
    },
  ];
}
