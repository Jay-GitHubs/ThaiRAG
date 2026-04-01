import { Button, Tooltip } from 'antd';
import { QuestionCircleOutlined } from '@ant-design/icons';
import { useI18n } from '../i18n';
import { useTour } from './useTour';

interface TourGuideButtonProps {
  tourId: string;
  style?: React.CSSProperties;
}

export function TourGuideButton({ tourId, style }: TourGuideButtonProps) {
  const { t } = useI18n();
  const { start } = useTour(tourId);

  return (
    <Tooltip title={t('tour.startGuide')}>
      <Button
        type="text"
        icon={<QuestionCircleOutlined />}
        onClick={start}
        style={style}
        data-tour="guide-button"
      />
    </Tooltip>
  );
}
