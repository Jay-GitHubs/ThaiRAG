import type { TourStepProps } from 'antd';

export function getSettingsSteps(t: (k: string) => string): TourStepProps[] {
  return [
    {
      title: t('tour.settings.step1.title'),
      description: t('tour.settings.step1.desc'),
      target: () => document.querySelector('[data-tour="settings-scope"]') as HTMLElement,
      placement: 'bottom',
    },
    {
      title: t('tour.settings.step2.title'),
      description: t('tour.settings.step2.desc'),
      target: () => document.querySelector('[data-tour="settings-tabs"]') as HTMLElement,
      placement: 'bottom',
    },
    {
      title: t('tour.settings.step3.title'),
      description: t('tour.settings.step3.desc'),
      target: () => document.querySelector('[data-tour="settings-presets"]') as HTMLElement,
      placement: 'top',
    },
    {
      title: t('tour.settings.step4.title'),
      description: t('tour.settings.step4.desc'),
      target: () => document.querySelector('[data-tour="settings-content"]') as HTMLElement,
      placement: 'top',
    },
    {
      title: t('tour.settings.step5.title'),
      description: t('tour.settings.step5.desc'),
      target: null as unknown as undefined,
      placement: 'center',
    },
  ];
}
