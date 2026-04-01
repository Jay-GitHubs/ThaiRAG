import { useState, useEffect, useContext } from 'react';
import { Layout, Menu, Typography, Button, Dropdown, Drawer, Grid, theme } from 'antd';
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
  MenuOutlined,
  SwapOutlined,
  StopOutlined,
  SearchOutlined,
  AuditOutlined,
  CodeOutlined,
  DatabaseOutlined,
  AppstoreOutlined,
  RobotOutlined,
} from '@ant-design/icons';
import type { MenuProps } from 'antd';
import { Outlet, useLocation, useNavigate } from 'react-router-dom';
import { useAuth } from '../auth/useAuth';
import { useThemeMode } from '../theme/ThemeContext';
import { useI18n } from '../i18n';
import type { Locale } from '../i18n';
import { QuestionCircleOutlined } from '@ant-design/icons';
import type { UserRole } from '../api/types';
import { GuidesDrawer } from '../tours';
import { TourContext } from '../tours';

const { Sider, Header, Content } = Layout;
const { useBreakpoint } = Grid;

// Role hierarchy: super_admin > admin > editor > viewer
const ROLE_LEVEL: Record<UserRole, number> = {
  super_admin: 4,
  admin: 3,
  editor: 2,
  viewer: 1,
};

type MenuItem = { key: string; icon: React.ReactNode; labelKey: string; minRole: UserRole };
type MenuGroup = {
  groupKey: string;
  labelKey: string;
  icon: React.ReactNode;
  items: MenuItem[];
};

// Top-level items (no group)
const topLevelItems: MenuItem[] = [
  { key: '/', icon: <DashboardOutlined />, labelKey: 'menu.dashboard', minRole: 'viewer' },
  { key: '/system', icon: <HeartOutlined />, labelKey: 'menu.health', minRole: 'viewer' },
];

// Grouped menu items by use case
const menuGroups: MenuGroup[] = [
  {
    groupKey: 'content',
    labelKey: 'menu.group.content',
    icon: <DatabaseOutlined />,
    items: [
      { key: '/km', icon: <ApartmentOutlined />, labelKey: 'menu.kmHierarchy', minRole: 'editor' },
      { key: '/documents', icon: <FileTextOutlined />, labelKey: 'menu.documents', minRole: 'editor' },
      { key: '/knowledge-graph', icon: <NodeIndexOutlined />, labelKey: 'menu.knowledgeGraph', minRole: 'editor' },
    ],
  },
  {
    groupKey: 'ai',
    labelKey: 'menu.group.ai',
    icon: <RobotOutlined />,
    items: [
      { key: '/finetune', icon: <ExperimentOutlined />, labelKey: 'menu.finetune', minRole: 'super_admin' },
      { key: '/prompt-marketplace', icon: <CodeOutlined />, labelKey: 'menu.promptMarketplace', minRole: 'editor' },
    ],
  },
  {
    groupKey: 'chatSearch',
    labelKey: 'menu.group.chatSearch',
    icon: <MessageOutlined />,
    items: [
      { key: '/test-chat', icon: <MessageOutlined />, labelKey: 'menu.testChat', minRole: 'editor' },
      { key: '/search-analytics', icon: <SearchOutlined />, labelKey: 'menu.searchAnalytics', minRole: 'admin' },
      { key: '/lineage', icon: <ApartmentOutlined />, labelKey: 'menu.lineage', minRole: 'admin' },
    ],
  },
  {
    groupKey: 'analytics',
    labelKey: 'menu.group.analytics',
    icon: <LineChartOutlined />,
    items: [
      { key: '/analytics', icon: <LineChartOutlined />, labelKey: 'menu.analytics', minRole: 'admin' },
      { key: '/usage', icon: <BarChartOutlined />, labelKey: 'menu.usageCosts', minRole: 'admin' },
      { key: '/feedback', icon: <FundOutlined />, labelKey: 'menu.feedbackTuning', minRole: 'admin' },
      { key: '/inference-logs', icon: <FileSearchOutlined />, labelKey: 'menu.inferenceLogs', minRole: 'super_admin' },
      { key: '/eval', icon: <ExperimentOutlined />, labelKey: 'menu.searchEval', minRole: 'super_admin' },
      { key: '/ab-tests', icon: <SplitCellsOutlined />, labelKey: 'menu.abTesting', minRole: 'super_admin' },
    ],
  },
  {
    groupKey: 'access',
    labelKey: 'menu.group.access',
    icon: <SafetyOutlined />,
    items: [
      { key: '/users', icon: <TeamOutlined />, labelKey: 'menu.users', minRole: 'admin' },
      { key: '/permissions', icon: <SafetyOutlined />, labelKey: 'menu.permissions', minRole: 'admin' },
      { key: '/tenants', icon: <AppstoreOutlined />, labelKey: 'menu.tenants', minRole: 'super_admin' },
      { key: '/roles', icon: <SafetyOutlined />, labelKey: 'menu.roles', minRole: 'super_admin' },
    ],
  },
  {
    groupKey: 'system',
    labelKey: 'menu.group.system',
    icon: <SettingOutlined />,
    items: [
      { key: '/settings', icon: <SettingOutlined />, labelKey: 'menu.settings', minRole: 'super_admin' },
      { key: '/connectors', icon: <ApiOutlined />, labelKey: 'menu.connectors', minRole: 'super_admin' },
      { key: '/backup', icon: <CloudDownloadOutlined />, labelKey: 'menu.backupRestore', minRole: 'super_admin' },
      { key: '/rate-limits', icon: <StopOutlined />, labelKey: 'menu.rateLimits', minRole: 'super_admin' },
      { key: '/vector-migration', icon: <SwapOutlined />, labelKey: 'menu.vectorMigration', minRole: 'super_admin' },
      { key: '/audit-log', icon: <AuditOutlined />, labelKey: 'menu.auditLog', minRole: 'super_admin' },
      { key: '/plugins', icon: <AppstoreOutlined />, labelKey: 'menu.plugins', minRole: 'super_admin' },
    ],
  },
];

const languageOptions: { key: Locale; label: string }[] = [
  { key: 'en', label: 'EN English' },
  { key: 'th', label: 'TH ไทย' },
];

export function AdminLayout() {
  const [collapsed, setCollapsed] = useState(false);
  const [drawerOpen, setDrawerOpen] = useState(false);
  const navigate = useNavigate();
  const location = useLocation();
  const { user, logout } = useAuth();
  const { mode, toggle: toggleTheme } = useThemeMode();
  const { token: themeToken } = theme.useToken();
  const { locale, setLocale, t } = useI18n();
  const tourCtx = useContext(TourContext);
  const screens = useBreakpoint();

  const isMobile = !screens.lg;

  // Close drawer on route change
  useEffect(() => {
    setDrawerOpen(false);
  }, [location.pathname]);

  const userRole = user?.role ?? 'viewer';
  const userLevel = ROLE_LEVEL[userRole] ?? 1;

  // Build grouped menu items
  const menuItems: MenuProps['items'] = [];

  // Add top-level items first
  for (const item of topLevelItems) {
    if (userLevel >= ROLE_LEVEL[item.minRole]) {
      menuItems.push({ key: item.key, icon: item.icon, label: t(item.labelKey) });
    }
  }

  // Add grouped items as collapsible sub-menus
  for (const group of menuGroups) {
    const visibleChildren = group.items.filter(
      (item) => userLevel >= ROLE_LEVEL[item.minRole],
    );
    if (visibleChildren.length === 0) continue;
    menuItems.push({
      key: group.groupKey,
      icon: group.icon,
      label: t(group.labelKey),
      children: visibleChildren.map((item) => ({
        key: item.key,
        icon: item.icon,
        label: t(item.labelKey),
      })),
    });
  }

  // Find selected key across all items (flat + grouped)
  const allFlatKeys = [
    ...topLevelItems.map((i) => i.key),
    ...menuGroups.flatMap((g) => g.items.map((i) => i.key)),
  ];
  const selectedKey = allFlatKeys.find((key) => {
    if (key === '/') return location.pathname === '/';
    return location.pathname.startsWith(key);
  }) || '/';

  // Auto-open the group containing the selected key
  const openKeys = menuGroups
    .filter((g) => g.items.some((i) => i.key === selectedKey))
    .map((g) => g.groupKey);

  const handleMenuClick = ({ key }: { key: string }) => {
    navigate(key);
    if (isMobile) {
      setDrawerOpen(false);
    }
  };

  const sidebarContent = (
    <>
      <div style={{ padding: 16, textAlign: 'center' }}>
        <Typography.Text strong style={{ color: '#fff', fontSize: 18 }}>
          ThaiRAG Admin
        </Typography.Text>
      </div>
      <Menu
        theme="dark"
        mode="inline"
        selectedKeys={[selectedKey]}
        defaultOpenKeys={openKeys}
        items={menuItems}
        onClick={handleMenuClick}
      />
    </>
  );

  return (
    <Layout style={{ minHeight: '100vh' }}>
      {/* Desktop sidebar */}
      {!isMobile && (
        <Sider collapsible collapsed={collapsed} onCollapse={setCollapsed} data-tour="sidebar">
          <div style={{ padding: 16, textAlign: 'center' }}>
            <Typography.Text strong style={{ color: '#fff', fontSize: collapsed ? 14 : 18 }}>
              {collapsed ? 'TR' : 'ThaiRAG Admin'}
            </Typography.Text>
          </div>
          <Menu
            theme="dark"
            mode="inline"
            selectedKeys={[selectedKey]}
            defaultOpenKeys={openKeys}
            items={menuItems}
            onClick={handleMenuClick}
          />
        </Sider>
      )}

      {/* Mobile drawer sidebar */}
      {isMobile && (
        <Drawer
          placement="left"
          open={drawerOpen}
          onClose={() => setDrawerOpen(false)}
          width={screens.xs ? 240 : 280}
          styles={{
            body: { padding: 0, backgroundColor: '#001529' },
            header: { display: 'none' },
          }}
        >
          {sidebarContent}
        </Drawer>
      )}

      <Layout>
        <Header
          style={{
            padding: '0 16px',
            background: themeToken.colorBgContainer,
            display: 'flex',
            justifyContent: 'space-between',
            alignItems: 'center',
            gap: 8,
          }}
        >
          {/* Left side: hamburger on mobile */}
          <div style={{ display: 'flex', alignItems: 'center' }}>
            {isMobile && (
              <Button
                type="text"
                icon={<MenuOutlined />}
                onClick={() => setDrawerOpen(true)}
                style={{ fontSize: 18, marginRight: 8 }}
                className="mobile-menu-btn"
              />
            )}
          </div>

          {/* Right side: user info + controls */}
          <div style={{ display: 'flex', alignItems: 'center', gap: isMobile ? 4 : 16 }}>
            {user && (
              <Typography.Text className="header-user-text">
                {t('header.loggedInAs', { email: user.email })}
              </Typography.Text>
            )}
            <Button
              icon={<QuestionCircleOutlined />}
              onClick={() => tourCtx.setGuidesOpen(true)}
              title={t('tour.guidesTitle')}
              size={isMobile ? 'small' : 'middle'}
              data-tour="guides-button"
            />
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
              <Button icon={<GlobalOutlined />} size={isMobile ? 'small' : 'middle'}>
                {!isMobile && locale.toUpperCase()}
              </Button>
            </Dropdown>
            <Button
              icon={mode === 'dark' ? <SunOutlined /> : <MoonOutlined />}
              onClick={toggleTheme}
              title={mode === 'dark' ? t('header.lightMode') : t('header.darkMode')}
              size={isMobile ? 'small' : 'middle'}
            />
            <Button icon={<LogoutOutlined />} onClick={logout} size={isMobile ? 'small' : 'middle'}>
              <span className="header-btn-text">{t('header.logout')}</span>
            </Button>
          </div>
        </Header>
        <Content style={{ margin: isMobile ? 12 : 24 }}>
          <Outlet />
        </Content>
      </Layout>
      <GuidesDrawer />
    </Layout>
  );
}
