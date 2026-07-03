import { Alert, Button, Descriptions, Divider, Form, Input, Modal, message } from 'antd';
import { useState } from 'react';
import { useAuth } from '../auth/AuthContext';
import { changePassword } from '../api/auth';
import { useI18n } from '../i18n/LocaleProvider';

/** Account settings: profile facts + self-service password change. Password
 *  change is offered only to native ('local') accounts — SSO users manage
 *  credentials at their IdP (the backend enforces this too). */
export function SettingsModal({ open, onClose }: { open: boolean; onClose: () => void }) {
  const { user } = useAuth();
  const { t } = useI18n();
  const [form] = Form.useForm();
  const [saving, setSaving] = useState(false);
  // Undefined auth_provider (sessions stored before the field existed) is
  // treated as local — the backend still rejects SSO accounts server-side.
  const isSso = !!user?.auth_provider && user.auth_provider !== 'local';

  const submit = async (values: { current: string; next: string; confirm: string }) => {
    if (values.next !== values.confirm) {
      form.setFields([{ name: 'confirm', errors: [t('passwordMismatch')] }]);
      return;
    }
    setSaving(true);
    try {
      await changePassword(values.current, values.next);
      message.success(t('passwordChanged'));
      form.resetFields();
      onClose();
    } catch (e) {
      message.error(e instanceof Error ? e.message : t('changePassword'));
    } finally {
      setSaving(false);
    }
  };

  return (
    <Modal open={open} onCancel={onClose} footer={null} title={t('settings')} width={440}>
      <Descriptions column={1} size="small" title={t('account')} style={{ marginTop: 8 }}>
        <Descriptions.Item label={t('nameLabel')}>{user?.name ?? '—'}</Descriptions.Item>
        <Descriptions.Item label={t('email')}>{user?.email ?? '—'}</Descriptions.Item>
      </Descriptions>
      <Divider style={{ margin: '14px 0' }} />
      {isSso ? (
        <Alert type="info" showIcon message={t('ssoPasswordNote')} />
      ) : (
        <Form form={form} layout="vertical" onFinish={submit} requiredMark={false}>
          <div className="eyebrow" style={{ marginBottom: 10 }}>
            {t('changePassword')}
          </div>
          <Form.Item
            label={t('currentPassword')}
            name="current"
            rules={[{ required: true, message: t('passwordRequired') }]}
          >
            <Input.Password autoComplete="current-password" data-testid="current-password" />
          </Form.Item>
          <Form.Item
            label={t('newPassword')}
            name="next"
            extra={t('passwordPolicyHint')}
            rules={[{ required: true, message: t('passwordRequired') }]}
          >
            <Input.Password autoComplete="new-password" data-testid="new-password" />
          </Form.Item>
          <Form.Item
            label={t('confirmPassword')}
            name="confirm"
            rules={[{ required: true, message: t('passwordRequired') }]}
          >
            <Input.Password autoComplete="new-password" data-testid="confirm-password" />
          </Form.Item>
          <Button
            type="primary"
            htmlType="submit"
            block
            loading={saving}
            data-testid="save-password"
          >
            {t('changePassword')}
          </Button>
        </Form>
      )}
    </Modal>
  );
}
