import { useContext } from 'react';
import { Drawer, List, Button, Tag, Typography, Divider } from 'antd';
import { CheckCircleOutlined, PlayCircleOutlined, UndoOutlined } from '@ant-design/icons';
import { useNavigate } from 'react-router-dom';
import { useI18n } from '../i18n';
import { useAuth } from '../auth/useAuth';
import type { UserRole } from '../api/types';
import { TourContext } from './TourContext';
import { TOUR_REGISTRY, GROUP_LABELS, type TourRegistration } from './registry';

const ROLE_LEVEL: Record<UserRole, number> = {
  super_admin: 4,
  admin: 3,
  editor: 2,
  viewer: 1,
};

export function GuidesDrawer() {
  const { t } = useI18n();
  const { user } = useAuth();
  const navigate = useNavigate();
  const ctx = useContext(TourContext);

  const userLevel = ROLE_LEVEL[user?.role ?? 'viewer'] ?? 1;

  // Filter tours by role and group them
  const visibleTours = TOUR_REGISTRY.filter(
    (tour) => userLevel >= ROLE_LEVEL[tour.minRole],
  );

  const groups = Array.from(new Set(visibleTours.map((t) => t.group)));

  const handleStart = (tour: TourRegistration) => {
    navigate(tour.pageRoute);
    // Small delay so the page renders before the tour starts
    setTimeout(() => ctx.startTour(tour.tourId), 300);
  };

  return (
    <Drawer
      title={t('tour.guidesTitle')}
      open={ctx.isGuidesOpen}
      onClose={() => ctx.setGuidesOpen(false)}
      width={400}
    >
      {groups.map((group) => {
        const toursInGroup = visibleTours.filter((t) => t.group === group);
        return (
          <div key={group}>
            <Divider orientation="left" orientationMargin={0}>
              <Typography.Text strong>{t(GROUP_LABELS[group])}</Typography.Text>
            </Divider>
            <List
              size="small"
              dataSource={toursInGroup}
              renderItem={(tour) => {
                const completed = ctx.isTourCompleted(tour.tourId);
                return (
                  <List.Item
                    actions={[
                      completed ? (
                        <Button
                          key="reset"
                          size="small"
                          icon={<UndoOutlined />}
                          onClick={() => ctx.resetTour(tour.tourId)}
                        >
                          {t('tour.reset')}
                        </Button>
                      ) : (
                        <Button
                          key="start"
                          size="small"
                          type="primary"
                          icon={<PlayCircleOutlined />}
                          onClick={() => handleStart(tour)}
                        >
                          {t('tour.start')}
                        </Button>
                      ),
                    ]}
                  >
                    <List.Item.Meta
                      title={
                        <span>
                          {t(tour.labelKey)}{' '}
                          {completed && (
                            <CheckCircleOutlined style={{ color: '#52c41a', marginLeft: 4 }} />
                          )}
                        </span>
                      }
                      description={
                        <>
                          {t(tour.descriptionKey)}{' '}
                          <Tag>{t('tour.stepsCount', { count: tour.stepCount })}</Tag>
                        </>
                      }
                    />
                  </List.Item>
                );
              }}
            />
          </div>
        );
      })}
      <Divider />
      <Button block danger onClick={ctx.resetAllTours}>
        {t('tour.resetAll')}
      </Button>
    </Drawer>
  );
}
