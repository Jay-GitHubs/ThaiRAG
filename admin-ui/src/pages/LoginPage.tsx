import { useEffect, useState } from 'react';
import { Card, Form, Input, Button, Typography, Divider, message } from 'antd';
import { LockOutlined, MailOutlined, SunOutlined, MoonOutlined } from '@ant-design/icons';
import { Navigate, useNavigate } from 'react-router-dom';
import { setToken } from '../api/client';
import { useAuth } from '../auth/useAuth';
import { useThemeMode } from '../theme/ThemeContext';
import { useEnabledProviders } from '../hooks/useEnabledProviders';

export function LoginPage() {
  const { login, isAuthenticated, loginWithToken } = useAuth();
  const navigate = useNavigate();
  const { mode, toggle: toggleTheme } = useThemeMode();
  const [loading, setLoading] = useState(false);
  const { data: providers } = useEnabledProviders();

  // Handle OIDC callback — token arrives via URL fragment
  useEffect(() => {
    const hash = window.location.hash;
    if (hash.includes('token=')) {
      const params = new URLSearchParams(hash.substring(1));
      const token = params.get('token');
      const userJson = params.get('user');
      if (token && userJson) {
        try {
          const user = JSON.parse(userJson);
          loginWithToken(token, user);
          // Clean up the hash
          window.history.replaceState(null, '', '/login');
          navigate('/');
        } catch {
          message.error('Failed to process SSO login');
        }
      }
    }
  }, [loginWithToken, navigate]);

  if (isAuthenticated) return <Navigate to="/" replace />;

  const onFinish = async (values: { email: string; password: string }) => {
    setLoading(true);
    try {
      await login(values.email, values.password);
      navigate('/');
    } catch (err: unknown) {
      const msg =
        err && typeof err === 'object' && 'response' in err
          ? (err as { response: { data?: { error?: { message?: string } } } }).response?.data
              ?.error?.message
          : undefined;
      message.error(msg || 'Login failed');
    } finally {
      setLoading(false);
    }
  };

  const providerColor: Record<string, string> = {
    oidc: '#52c41a',
    oauth2: '#722ed1',
    saml: '#fa8c16',
    ldap: '#13c2c2',
  };

  return (
    <Card
      style={{ width: 400 }}
      extra={
        <Button
          type="text"
          icon={mode === 'dark' ? <SunOutlined /> : <MoonOutlined />}
          onClick={toggleTheme}
          title={mode === 'dark' ? 'Switch to light mode' : 'Switch to dark mode'}
        />
      }
    >
      <Typography.Title level={3} style={{ textAlign: 'center', marginBottom: 24 }}>
        ThaiRAG Admin
      </Typography.Title>
      <Form layout="vertical" onFinish={onFinish}>
        <Form.Item name="email" rules={[{ required: true, message: 'Email required' }]}>
          <Input prefix={<MailOutlined />} placeholder="Email" size="large" />
        </Form.Item>
        <Form.Item name="password" rules={[{ required: true, message: 'Password required' }]}>
          <Input.Password prefix={<LockOutlined />} placeholder="Password" size="large" />
        </Form.Item>
        <Form.Item>
          <Button type="primary" htmlType="submit" block size="large" loading={loading}>
            Sign In
          </Button>
        </Form.Item>
      </Form>
      {providers && providers.length > 0 && (
        <>
          <Divider>or sign in with</Divider>
          {providers.map((p) => (
            <Button
              key={p.id}
              block
              size="large"
              style={{ marginBottom: 8, borderColor: providerColor[p.provider_type] }}
              onClick={() => {
                if (p.provider_type === 'oidc' || p.provider_type === 'oauth2') {
                  window.location.href = `/api/auth/oauth/${p.id}/authorize`;
                } else if (p.provider_type === 'ldap') {
                  message.info('LDAP login form is not yet implemented');
                } else {
                  message.info(`${p.provider_type.toUpperCase()} login is not yet implemented`);
                }
              }}
            >
              {p.name} ({p.provider_type.toUpperCase()})
            </Button>
          ))}
        </>
      )}
    </Card>
  );
}
