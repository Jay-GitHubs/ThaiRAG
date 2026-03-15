import { Alert, Typography } from 'antd';

export function LocalAuthTab() {
  return (
    <>
      <Alert
        type="info"
        showIcon
        message="Local Authentication"
        description="Local auth uses email/password with Argon2 hashing and JWT tokens."
        style={{ marginBottom: 16 }}
      />
      <Typography.Paragraph>
        Local authentication is configured via environment variables:
      </Typography.Paragraph>
      <ul>
        <li>
          <Typography.Text code>THAIRAG__AUTH__ENABLED</Typography.Text> — enable/disable auth
        </li>
        <li>
          <Typography.Text code>THAIRAG__AUTH__JWT_SECRET</Typography.Text> — JWT signing secret
        </li>
        <li>
          <Typography.Text code>THAIRAG__AUTH__TOKEN_EXPIRY_HOURS</Typography.Text> — token lifetime
        </li>
        <li>
          <Typography.Text code>THAIRAG__ADMIN__EMAIL</Typography.Text> / <Typography.Text code>THAIRAG__ADMIN__PASSWORD</Typography.Text> — super admin seeding on startup
        </li>
      </ul>
    </>
  );
}
