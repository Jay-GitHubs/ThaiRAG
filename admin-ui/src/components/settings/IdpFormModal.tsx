import { useEffect } from 'react';
import { Form, Input, Modal, Select, Switch } from 'antd';
import type { IdentityProvider, IdpType } from '../../api/types';

interface Props {
  open: boolean;
  editingIdp: IdentityProvider | null;
  onCancel: () => void;
  onSubmit: (values: {
    name: string;
    provider_type: IdpType;
    enabled: boolean;
    config: Record<string, unknown>;
  }) => void;
  loading: boolean;
}

const idpTypes: { value: IdpType; label: string }[] = [
  { value: 'oidc', label: 'OIDC' },
  { value: 'oauth2', label: 'OAuth2' },
  { value: 'saml', label: 'SAML' },
  { value: 'ldap', label: 'LDAP' },
];

const oidcFields = ['issuer_url', 'client_id', 'client_secret', 'scopes', 'redirect_uri'];
const oauth2Fields = [
  'authorize_url',
  'token_url',
  'userinfo_url',
  'client_id',
  'client_secret',
  'scopes',
  'redirect_uri',
];
const samlFields = ['idp_entity_id', 'sso_url', 'slo_url', 'certificate', 'sp_entity_id'];
const ldapFields = ['server_url', 'bind_dn', 'bind_password', 'search_base', 'search_filter'];

const secretFields = ['client_secret', 'bind_password', 'certificate'];

function configFieldsFor(type: IdpType | undefined) {
  switch (type) {
    case 'oidc':
      return oidcFields;
    case 'oauth2':
      return oauth2Fields;
    case 'saml':
      return samlFields;
    case 'ldap':
      return ldapFields;
    default:
      return [];
  }
}

export function IdpFormModal({ open, editingIdp, onCancel, onSubmit, loading }: Props) {
  const [form] = Form.useForm();
  const providerType = Form.useWatch('provider_type', form);

  useEffect(() => {
    if (open) {
      if (editingIdp) {
        const { role_mapping, ...restConfig } = editingIdp.config as Record<string, unknown>;
        form.setFieldsValue({
          name: editingIdp.name,
          provider_type: editingIdp.provider_type,
          enabled: editingIdp.enabled,
          ...restConfig,
          role_mapping: role_mapping ? JSON.stringify(role_mapping, null, 2) : '',
        });
      } else {
        form.resetFields();
        form.setFieldsValue({ enabled: true });
      }
    }
  }, [open, editingIdp, form]);

  const handleOk = async () => {
    const values = await form.validateFields();
    const { name, provider_type, enabled, role_mapping, ...configValues } = values;
    const config: Record<string, unknown> = { ...configValues };
    if (role_mapping) {
      config.role_mapping = JSON.parse(role_mapping);
    }
    onSubmit({ name, provider_type, enabled, config });
  };

  const fields = configFieldsFor(providerType);

  return (
    <Modal
      title={editingIdp ? 'Edit Identity Provider' : 'Add Identity Provider'}
      open={open}
      onOk={handleOk}
      onCancel={onCancel}
      confirmLoading={loading}
      destroyOnClose
    >
      <Form form={form} layout="vertical">
        <Form.Item name="name" label="Name" rules={[{ required: true }]}>
          <Input />
        </Form.Item>
        <Form.Item name="provider_type" label="Type" rules={[{ required: true }]}>
          <Select options={idpTypes} />
        </Form.Item>
        <Form.Item name="enabled" label="Enabled" valuePropName="checked">
          <Switch />
        </Form.Item>
        {fields.map((field) => (
          <Form.Item key={field} name={field} label={field.replace(/_/g, ' ')}>
            {field === 'certificate' ? (
              <Input.TextArea rows={4} />
            ) : secretFields.includes(field) ? (
              <Input.Password />
            ) : field === 'tls_enabled' ? (
              <Switch />
            ) : (
              <Input />
            )}
          </Form.Item>
        ))}
        {providerType === 'ldap' && (
          <Form.Item name="tls_enabled" label="TLS Enabled" valuePropName="checked">
            <Switch />
          </Form.Item>
        )}
        {(providerType === 'oidc' || providerType === 'oauth2') && (
          <Form.Item
            name="role_mapping"
            label="Role Mapping (JSON)"
            tooltip='Maps IdP roles to ThaiRAG roles. Example: {"thairag-admin": "admin", "thairag-editor": "editor"}'
            rules={[
              {
                validator: (_, value) => {
                  if (!value) return Promise.resolve();
                  try {
                    const parsed = JSON.parse(value);
                    if (typeof parsed !== 'object' || Array.isArray(parsed)) {
                      return Promise.reject('Must be a JSON object');
                    }
                    return Promise.resolve();
                  } catch {
                    return Promise.reject('Invalid JSON');
                  }
                },
              },
            ]}
          >
            <Input.TextArea
              rows={3}
              placeholder='{"keycloak-role": "admin", "keycloak-editor": "editor"}'
            />
          </Form.Item>
        )}
      </Form>
    </Modal>
  );
}
