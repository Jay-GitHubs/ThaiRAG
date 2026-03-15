import { Layout, theme } from 'antd';
import { Outlet } from 'react-router-dom';

export function AuthLayout() {
  const { token } = theme.useToken();
  return (
    <Layout
      style={{
        minHeight: '100vh',
        justifyContent: 'center',
        alignItems: 'center',
        background: token.colorBgLayout,
      }}
    >
      <Outlet />
    </Layout>
  );
}
