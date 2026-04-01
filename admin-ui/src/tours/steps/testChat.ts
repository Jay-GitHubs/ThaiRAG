import type { TourStepProps } from 'antd';

export function getTestChatSteps(t: (k: string) => string): TourStepProps[] {
  return [
    {
      title: t('tour.testChat.step1.title'),
      description: t('tour.testChat.step1.desc'),
      target: () => document.querySelector('[data-tour="chat-ws-select"]') as HTMLElement,
      placement: 'bottom',
    },
    {
      title: t('tour.testChat.step2.title'),
      description: t('tour.testChat.step2.desc'),
      target: () => document.querySelector('[data-tour="chat-input"]') as HTMLElement,
      placement: 'top',
    },
    {
      title: t('tour.testChat.step3.title'),
      description: t('tour.testChat.step3.desc'),
      target: () => document.querySelector('[data-tour="chat-send"]') as HTMLElement,
      placement: 'top',
    },
    {
      title: t('tour.testChat.step4.title'),
      description: t('tour.testChat.step4.desc'),
      target: () => document.querySelector('[data-tour="chat-response"]') as HTMLElement,
      placement: 'top',
    },
    {
      title: t('tour.testChat.step5.title'),
      description: t('tour.testChat.step5.desc'),
      target: () => document.querySelector('[data-tour="chat-pipeline"]') as HTMLElement,
      placement: 'top',
    },
    {
      title: t('tour.testChat.step6.title'),
      description: t('tour.testChat.step6.desc'),
      target: null as unknown as undefined,
      placement: 'center',
    },
  ];
}
