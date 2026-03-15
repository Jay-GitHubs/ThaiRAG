import { Modal, Form, Input, Select, message } from 'antd';
import { useIngestDocument } from '../../hooks/useDocuments';

interface Props {
  workspaceId: string;
  open: boolean;
  onClose: () => void;
}

const mimeOptions = [
  { label: 'Plain Text', value: 'text/plain' },
  { label: 'Markdown', value: 'text/markdown' },
  { label: 'HTML', value: 'text/html' },
  { label: 'CSV', value: 'text/csv' },
];

export function IngestModal({ workspaceId, open, onClose }: Props) {
  const [form] = Form.useForm();
  const ingest = useIngestDocument();

  async function handleOk() {
    try {
      const values = await form.validateFields();
      const res = await ingest.mutateAsync({
        wsId: workspaceId,
        data: {
          title: values.title,
          content: values.content,
          mime_type: values.mime_type,
        },
      });
      message.success(`Ingested: ${res.chunks} chunks indexed`);
      form.resetFields();
      onClose();
    } catch {
      message.error('Ingest failed');
    }
  }

  return (
    <Modal
      title="Ingest Text Document"
      open={open}
      onOk={handleOk}
      onCancel={onClose}
      confirmLoading={ingest.isPending}
      width={600}
    >
      <Form form={form} layout="vertical" initialValues={{ mime_type: 'text/plain' }}>
        <Form.Item name="title" label="Title" rules={[{ required: true }]}>
          <Input placeholder="Document title" />
        </Form.Item>
        <Form.Item name="mime_type" label="MIME Type">
          <Select options={mimeOptions} />
        </Form.Item>
        <Form.Item name="content" label="Content" rules={[{ required: true }]}>
          <Input.TextArea rows={10} placeholder="Paste document content here..." />
        </Form.Item>
      </Form>
    </Modal>
  );
}
