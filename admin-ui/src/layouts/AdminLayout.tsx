import { useState } from 'react';
import { Layout, Menu, Typography, Button, Dropdown, theme } from 'antd';
import {
  DashboardOutlined,
  ApartmentOutlined,
  FileTextOutlined,
  MessageOutlined,
  BarChartOutlined,
  LineChartOutlined,
  TeamOutlined,
  SafetyOutlined,
  ApiOutlined,
  SettingOutlined,
  HeartOutlined,
  FundOutlined,
  FileSearchOutlined,
  ExperimentOutlined,
  SplitCellsOutlined,
  CloudDownloadOutlined,
  NodeIndexOutlined,
  LogoutOutlined,
  SunOutlined,
  MoonOutlined,
  GlobalOutlined,
} from '@ant-design/icons';
import { Outlet, useLocation, useNavigate } from 'react-router-dom';
import { useAuth } from '../auth/useAuth';
import { useThemeMode } from '../theme/ThemeContext';
import { useI18n } from '../i18n';
import type { Locale } from '../i18n';
import type { UserRole } from '../api/types';

const { Sider, Header, Content } = Layout;

// Role hierarchy: super_admin > admin > editor > viewer
const ROLE_LEVEL: Record<UserRole, number> = {
  super_admin: 4,
  admin: 3,
  editor: 2,
  viewer: 1,
};

// Menu items with translation keys instead of hardcoded labels
const baseMenuItems: { key: string; icon: React.ReactNode; labelKey: string; minRole: UserRole }[] = [
  { key: '/', icon: <DashboardOutlined />, labelKey: 'menu.dashboard', minRole: 'viewer' },
  { key: '/km', icon: <ApartmentOutlined />, labelKey: 'menu.kmHierarchy', minRole: 'editor' },
  { key: '/documents', icon: <FileTextOutlined />, labelKey: 'menu.documents', minRole: 'editor' },
  { key: '/knowledge-graph', icon: <NodeIndexOutlined />, labelKey: 'menu.knowledgeGraph', minRole: 'editor' },
  { key: '/test-chat', icon: <MessageOutlined />, labelKey: 'menu.testChat', minRole: 'editor' },
  { key: '/users', icon: <TeamOutlined />, labelKey: 'menu.users', minRole: 'admin' },
  { key: '/permissions', icon: <SafetyOutlined />, labelKey: 'menu.permissions', minRole: 'admin' },
  { key: '/usage', icon: <BarChartOutlined />, labelKey: 'menu.usageCosts', minRole: 'admin' },
  { key: '/feedback', icon: <FundOutlined />, labelKey: 'menu.feedbackTuning', minRole: 'admin' },
  { key: '/analytics', icon: <LineChartOutlined />, labelKey: 'menu.analytics', minRole: 'admin' },
  { key: '/connectors', icon: <ApiOutlined />, labelKey: 'menu.connectors', minRole: 'super_admin' },
  { key: '/inference-logs', icon: <FileSearchOutlined />, labelKey: 'menu.inferenceLogs', minRole: 'super_admin' },
  { key: '/eval', icon: <ExperimentOutlined />, labelKey: 'menu.searchEval', minRole: 'super_admin' },
  { key: '/ab-tests', icon: <SplitCellsOutlined />, labelKey: 'menu.abTesting', minRole: 'super_admin' },
  { key: '/backup', icon: <CloudDownloadOutlined />, labelKey: 'menu.backupRestore', minRole: 'super_admin' },
  { key: '/settings', icon: <SettingOutlined />, labelKey: 'menu.settings', minRole: 'super_admin' },
  { key: '/system', icon: <HeartOutlined />, labelKey: 'menu.health', minRole: 'viewer' },
];

const languageOptions: { key: Locale; label: string }[] = [
  { key: 'en', label: 'EN English' },
  { key: 'th', label: 'TH ไทย' },
];

export function AdminLayout() {
  const [collapsed, setCollapsed] = useState(false);
  const navigate = useNavigate();
  const location = useLocation();
  const { user, logout } = useAuth();
  const { mode, toggle: toggleTheme } = useThemeMode();
  const { token: themeToken } = theme.useToken();
  const { locale, setLocale, t } = useI18n();

  const userRole = user?.role ?? 'viewer';
  const userLevel = ROLE_LEVEL[userRole] ?? 1;

  const menuItems = baseMenuItems
    .filter((item) => userLevel >= ROLE_LEVEL[item.minRole])
    .map((item) => ({
      key: item.key,
      icon: item.icon,
      label: t(item.labelKey),
    }));

  const selectedKey = menuItems.find((item) => {
    if (item.key === '/') return location.pathname === '/';
    return location.pathname.startsWith(item.key);
  })?.key || '/';

  return (
    <Layout style={{ minHeight: '100vh' }}>
      <Sider collapsible collapsed={collapsed} onCollapse={setCollapsed}>
        <div style={{ padding: 16, textAlign: 'center' }}>
          <Typography.Text strong style={{ color: '#fff', fontSize: collapsed ? 14 : 18 }}>
            {collapsed ? 'TR' : 'ThaiRAG Admin'}
          </Typography.Text>
        </div>
        <Menu
          theme="dark"
          mode="inline"
          selectedKeys={[selectedKey]}
          items={menuItems}
          onClick={({ key }) => navigate(key)}
        />
      </Sider>
      <Layout>
        <Header
          style={{
            padding: '0 24px',
            background: themeToken.colorBgContainer,
            display: 'flex',
            justifyContent: 'flex-end',
            alignItems: 'center',
            gap: 16,
          }}
        >
          {user && <Typography.Text>{t('header.loggedInAs', { email: user.email })}</Typography.Text>}
          <Dropdown
            menu={{
              items: languageOptions.map((opt) => ({
                key: opt.key,
                label: opt.label,
              })),
              selectedKeys: [locale],
              onClick: ({ key }) => setLocale(key as Locale),
            }}
            trigger={['click']}
          >
            <Button icon={<GlobalOutlined />}>
              {locale.toUpperCase()}
            </Button>
          </Dropdown>
          <Button
            icon={mode === 'dark' ? <SunOutlined /> : <MoonOutlined />}
            onClick={toggleTheme}
            title={mode === 'dark' ? t('header.lightMode') : t('header.darkMode')}
          />
          <Button icon={<LogoutOutlined />} onClick={logout}>
            {t('header.logout')}
          </Button>
        </Header>
        <Content style={{ margin: 24 }}>
          <Outlet />
        </Content>
      </Layout>
    </Layout>
  );
}
