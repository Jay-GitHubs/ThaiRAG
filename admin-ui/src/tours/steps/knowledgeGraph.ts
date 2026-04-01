import type { TourStepProps } from 'antd';

export function getKnowledgeGraphSteps(t: (k: string) => string): TourStepProps[] {
  return [
    {
      title: t('tour.knowledgeGraph.step1.title'),
      description: t('tour.knowledgeGraph.step1.desc'),
      target: () => document.querySelector('[data-tour="kg-ws-select"]') as HTMLElement,
      placement: 'bottom',
    },
    {
      title: t('tour.knowledgeGraph.step2.title'),
      description: t('tour.knowledgeGraph.step2.desc'),
      target: () => document.querySelector('[data-tour="kg-view-toggle"]') as HTMLElement,
      placement: 'bottom',
    },
    {
      title: t('tour.knowledgeGraph.step3.title'),
      description: t('tour.knowledgeGraph.step3.desc'),
      target: () => document.querySelector('[data-tour="kg-content"]') as HTMLElement,
      placement: 'top',
    },
  ];
}
