import { useEffect, useState } from 'react';
import { Card, Form, Input, Button, Upload, Space, Typography, message, Image } from 'antd';
import { UploadOutlined, DeleteOutlined } from '@ant-design/icons';
import type { UploadProps } from 'antd';
import { getBranding, updateBranding } from '../../api/branding';
import { apiErrorMessage } from '../../api/client';
import { useBranding } from '../../branding/BrandingProvider';

const MAX_LOGO_BYTES = 200 * 1024;

/** White-label the product name + logo. Applies to both the admin and chat
 *  UIs (they read the same public /api/branding). */
export function BrandingTab() {
  const { refresh } = useBranding();
  const [form] = Form.useForm();
  const [logo, setLogo] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    getBranding()
      .then((b) => {
        form.setFieldsValue({ app_name: b.app_name });
        setLogo(b.logo_data_url);
      })
      .catch(() => {});
  }, [form]);

  const beforeUpload: UploadProps['beforeUpload'] = (file) => {
    if (!file.type.startsWith('image/')) {
      message.error('Logo must be an image.');
      return Upload.LIST_IGNORE;
    }
    const reader = new FileReader();
    reader.onload = () => {
      const dataUrl = reader.result as string;
      if (dataUrl.length > MAX_LOGO_BYTES) {
        message.error(`Logo too large (max ${MAX_LOGO_BYTES / 1024} KB). Use a smaller image.`);
        return;
      }
      setLogo(dataUrl);
    };
    reader.readAsDataURL(file);
    return false; // never actually upload; we inline the data URL
  };

  const onSave = async (values: { app_name: string }) => {
    setSaving(true);
    try {
      await updateBranding({ app_name: values.app_name, logo_data_url: logo });
      await refresh();
      message.success('Branding saved — it applies across the admin and chat UIs.');
    } catch (err) {
      message.error(apiErrorMessage(err, 'Failed to save branding'));
    } finally {
      setSaving(false);
    }
  };

  return (
    <Card title="Branding (white-label)">
      <Typography.Paragraph type="secondary">
        Set the product name and logo shown to your users in both the admin console and the chat
        app. Read publicly so it also appears on the login screens.
      </Typography.Paragraph>
      <Form form={form} layout="vertical" onFinish={onSave} style={{ maxWidth: 480 }}>
        <Form.Item
          name="app_name"
          label="Product name"
          rules={[
            { required: true, message: 'Name is required' },
            { max: 60, message: 'Max 60 characters' },
          ]}
        >
          <Input placeholder="e.g. Acme Knowledge" />
        </Form.Item>

        <Form.Item label="Logo">
          <Space direction="vertical">
            {logo && (
              <Image src={logo} alt="logo preview" style={{ maxHeight: 48, background: '#0002', padding: 4 }} />
            )}
            <Space>
              <Upload beforeUpload={beforeUpload} showUploadList={false} accept="image/*">
                <Button icon={<UploadOutlined />}>Upload logo</Button>
              </Upload>
              {logo && (
                <Button danger icon={<DeleteOutlined />} onClick={() => setLogo(null)}>
                  Remove
                </Button>
              )}
            </Space>
            <Typography.Text type="secondary" style={{ fontSize: 12 }}>
              PNG or SVG, up to {MAX_LOGO_BYTES / 1024} KB. Transparent background recommended.
            </Typography.Text>
          </Space>
        </Form.Item>

        <Button type="primary" htmlType="submit" loading={saving}>
          Save branding
        </Button>
      </Form>
    </Card>
  );
}
