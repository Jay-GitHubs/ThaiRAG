import { useEffect, useState } from 'react';
import { Navigate, useNavigate } from 'react-router-dom';
import { Button, Divider, Form, Input, message } from 'antd';
import { useAuth } from '../auth/AuthContext';
import { listProviders } from '../api/auth';
import type { ProviderInfo } from '../api/types';
import { BrandMark } from '../components/BrandMark';

export function LoginPage() {
  const { login, loginWithToken, isAuthenticated } = useAuth();
  const navigate = useNavigate();
  const [loading, setLoading] = useState(false);
  const [providers, setProviders] = useState<ProviderInfo[]>([]);

  // OIDC/SSO callback: the backend redirects back with #token=…&user=… in the
  // URL fragment. Pick it up, store the session, and land in the app.
  useEffect(() => {
    const hash = window.location.hash;
    if (!hash.includes('token=')) return;
    const params = new URLSearchParams(hash.substring(1));
    const token = params.get('token');
    const userJson = params.get('user');
    if (token && userJson) {
      try {
        loginWithToken(token, JSON.parse(userJson));
        window.history.replaceState(null, '', '/login');
        navigate('/', { replace: true });
      } catch {
        message.error('Failed to complete SSO sign-in.');
      }
    }
  }, [loginWithToken, navigate]);

  // Enabled SSO providers → "Sign in with X" buttons (native login still works).
  useEffect(() => {
    listProviders()
      .then(setProviders)
      .catch(() => {
        /* no SSO buttons if this fails */
      });
  }, []);

  if (isAuthenticated) {
    return <Navigate to="/" replace />;
  }

  const onFinish = async (values: { email: string; password: string }) => {
    setLoading(true);
    try {
      await login(values.email, values.password);
      navigate('/', { replace: true });
    } catch {
      message.error('Those credentials did not match. Try again.');
    } finally {
      setLoading(false);
    }
  };

  const startSso = (p: ProviderInfo) => {
    if (p.provider_type === 'oidc' || p.provider_type === 'oauth2') {
      window.location.href = `/api/auth/oauth/${p.id}/authorize`;
    } else {
      message.info(`${p.provider_type.toUpperCase()} login isn't available here yet.`);
    }
  };

  return (
    <div style={{ height: '100%', display: 'flex' }}>
      {/* Ink narrative panel — the brand thesis, grounded in documents. */}
      <aside
        className="hide-narrow"
        style={{
          position: 'relative',
          flex: '1.1',
          background: 'var(--ink)',
          color: 'var(--ink-bright)',
          padding: '40px 56px',
          display: 'flex',
          flexDirection: 'column',
          justifyContent: 'space-between',
          overflow: 'hidden',
        }}
      >
        <div className="paper-stack">
          <span style={{ top: '8%' }} />
          <span style={{ top: '34%', right: '-140px' }} />
          <span style={{ top: '60%' }} />
        </div>
        <BrandMark tone="light" />
        <div style={{ position: 'relative', maxWidth: 460 }}>
          <div className="eyebrow" style={{ color: 'var(--ink-dim)', marginBottom: 18 }}>
            Document intelligence · เอกสารอัจฉริยะ
          </div>
          <h1
            style={{
              fontFamily: 'var(--font-display)',
              fontWeight: 600,
              fontSize: 40,
              lineHeight: 1.25,
              margin: 0,
            }}
          >
            Ask your documents.
            <br />
            <span style={{ color: 'var(--celadon)' }}>Answers with sources.</span>
          </h1>
          <p style={{ color: 'var(--ink-dim)', fontSize: 16, lineHeight: 1.7, marginTop: 18 }}>
            ถามเป็นภาษาไทยหรืออังกฤษ แล้วได้คำตอบพร้อมหน้าเอกสารต้นทาง — ไม่ต้องเปิดหาเอง.
          </p>
        </div>
        <div className="eyebrow" style={{ color: 'var(--ink-dim)' }}>
          Grounded in your knowledge base
        </div>
      </aside>

      {/* Form pane on warm paper. */}
      <main
        style={{
          width: 440,
          maxWidth: '100%',
          margin: '0 auto',
          display: 'flex',
          flexDirection: 'column',
          justifyContent: 'center',
          padding: '40px 44px',
          background: 'var(--canvas)',
        }}
      >
        <div style={{ marginBottom: 28 }}>
          <div className="eyebrow" style={{ marginBottom: 8 }}>
            Welcome back
          </div>
          <h2 style={{ fontFamily: 'var(--font-display)', fontWeight: 600, fontSize: 26, margin: 0 }}>
            Sign in to chat
          </h2>
        </div>
        <Form layout="vertical" onFinish={onFinish} requiredMark={false} size="large">
          <Form.Item
            label="Email"
            name="email"
            rules={[{ required: true, message: 'Email is required' }]}
          >
            <Input type="email" autoComplete="username" placeholder="you@company.co.th" />
          </Form.Item>
          <Form.Item
            label="Password"
            name="password"
            rules={[{ required: true, message: 'Password is required' }]}
          >
            <Input.Password autoComplete="current-password" placeholder="Your password" />
          </Form.Item>
          <Button type="primary" htmlType="submit" block loading={loading} style={{ marginTop: 4 }}>
            Sign in
          </Button>
        </Form>

        {providers.length > 0 && (
          <>
            <Divider plain style={{ color: 'var(--text-muted)', fontSize: 12 }}>
              or
            </Divider>
            {providers.map((p) => (
              <Button
                key={p.id}
                block
                size="large"
                style={{ marginBottom: 8 }}
                onClick={() => startSso(p)}
              >
                Continue with {p.name}
              </Button>
            ))}
          </>
        )}
      </main>
    </div>
  );
}
