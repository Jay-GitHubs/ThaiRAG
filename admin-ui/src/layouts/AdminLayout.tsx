import { useState } from 'react';
import { Layout, Menu, Typography, Button, theme } from 'antd';
import {
  DashboardOutlined,
  ApartmentOutlined,
  FileTextOutlined,
  MessageOutlined,
  BarChartOutlined,
  TeamOutlined,
  SafetyOutlined,
  SettingOutlined,
  HeartOutlined,
  FundOutlined,
  LogoutOutlined,
  SunOutlined,
  MoonOutlined,
} from '@ant-design/icons';
import { Outlet, useLocation, useNavigate } from 'react-router-dom';
import { useAuth } from '../auth/useAuth';
import { useThemeMode } from '../theme/ThemeContext';
import type { UserRole } from '../api/types';

const { Sider, Header, Content } = Layout;

// Role hierarchy: super_admin > admin > editor > viewer
const ROLE_LEVEL: Record<UserRole, number> = {
  super_admin: 4,
  admin: 3,
  editor: 2,
  viewer: 1,
};

const baseMenuItems = [
  { key: '/', icon: <DashboardOutlined />, label: 'Dashboard', minRole: 'viewer' as UserRole },
  { key: '/km', icon: <ApartmentOutlined />, label: 'KM Hierarchy', minRole: 'editor' as UserRole },
  { key: '/documents', icon: <FileTextOutlined />, label: 'Documents', minRole: 'editor' as UserRole },
  { key: '/test-chat', icon: <MessageOutlined />, label: 'Test Chat', minRole: 'editor' as UserRole },
  { key: '/users', icon: <TeamOutlined />, label: 'Users', minRole: 'admin' as UserRole },
  { key: '/permissions', icon: <SafetyOutlined />, label: 'Permissions', minRole: 'admin' as UserRole },
  { key: '/usage', icon: <BarChartOutlined />, label: 'Usage & Costs', minRole: 'admin' as UserRole },
  { key: '/feedback', icon: <FundOutlined />, label: 'Feedback & Tuning', minRole: 'admin' as UserRole },
  { key: '/settings', icon: <SettingOutlined />, label: 'Settings', minRole: 'super_admin' as UserRole },
  { key: '/system', icon: <HeartOutlined />, label: 'Health', minRole: 'viewer' as UserRole },
];

export function AdminLayout() {
  const [collapsed, setCollapsed] = useState(false);
  const navigate = useNavigate();
  const location = useLocation();
  const { user, logout } = useAuth();
  const { mode, toggle: toggleTheme } = useThemeMode();
  const { token: themeToken } = theme.useToken();

  const userRole = user?.role ?? 'viewer';
  const userLevel = ROLE_LEVEL[userRole] ?? 1;

  const menuItems = baseMenuItems.filter(
    (item) => userLevel >= ROLE_LEVEL[item.minRole],
  );

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
          {user && <Typography.Text>Logged in as {user.email}</Typography.Text>}
          <Button
            icon={mode === 'dark' ? <SunOutlined /> : <MoonOutlined />}
            onClick={toggleTheme}
            title={mode === 'dark' ? 'Switch to light mode' : 'Switch to dark mode'}
          />
          <Button icon={<LogoutOutlined />} onClick={logout}>
            Logout
          </Button>
        </Header>
        <Content style={{ margin: 24 }}>
          <Outlet />
        </Content>
      </Layout>
    </Layout>
  );
}
