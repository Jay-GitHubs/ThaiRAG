import type { TourStepProps } from 'antd';

export function getPromptsSteps(t: (k: string) => string): TourStepProps[] {
  return [
    {
      title: t('tour.prompts.step1.title'),
      description: t('tour.prompts.step1.desc'),
      target: () => document.querySelector('[data-tour="prompts-create"]') as HTMLElement,
      placement: 'bottom',
    },
    {
      title: t('tour.prompts.step2.title'),
      description: t('tour.prompts.step2.desc'),
      target: () => document.querySelector('[data-tour="prompts-search"]') as HTMLElement,
      placement: 'bottom',
    },
    {
      title: t('tour.prompts.step3.title'),
      description: t('tour.prompts.step3.desc'),
      target: () => document.querySelector('[data-tour="prompts-grid"]') as HTMLElement,
      placement: 'top',
    },
    {
      title: t('tour.prompts.step4.title'),
      description: t('tour.prompts.step4.desc'),
      target: null as unknown as undefined,
      placement: 'center',
    },
  ];
}
