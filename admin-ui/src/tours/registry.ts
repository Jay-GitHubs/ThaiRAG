import type { UserRole } from '../api/types';

export interface TourRegistration {
  tourId: string;
  labelKey: string;
  descriptionKey: string;
  pageRoute: string;
  minRole: UserRole;
  stepCount: number;
  group: 'top' | 'content' | 'chatSearch' | 'analytics' | 'access' | 'system';
}

export const TOUR_REGISTRY: TourRegistration[] = [
  // Top-level
  { tourId: 'dashboard', labelKey: 'tour.dashboard.title', descriptionKey: 'tour.dashboard.desc', pageRoute: '/', minRole: 'viewer', stepCount: 5, group: 'top' },

  // Content
  { tourId: 'km-hierarchy', labelKey: 'tour.km.title', descriptionKey: 'tour.km.desc', pageRoute: '/km', minRole: 'editor', stepCount: 4, group: 'content' },
  { tourId: 'documents', labelKey: 'tour.documents.title', descriptionKey: 'tour.documents.desc', pageRoute: '/documents', minRole: 'editor', stepCount: 5, group: 'content' },
  { tourId: 'knowledge-graph', labelKey: 'tour.knowledgeGraph.title', descriptionKey: 'tour.knowledgeGraph.desc', pageRoute: '/knowledge-graph', minRole: 'editor', stepCount: 3, group: 'content' },
  { tourId: 'prompts', labelKey: 'tour.prompts.title', descriptionKey: 'tour.prompts.desc', pageRoute: '/prompt-marketplace', minRole: 'editor', stepCount: 4, group: 'content' },

  // Chat & Search
  { tourId: 'test-chat', labelKey: 'tour.testChat.title', descriptionKey: 'tour.testChat.desc', pageRoute: '/test-chat', minRole: 'editor', stepCount: 6, group: 'chatSearch' },
  { tourId: 'search-analytics', labelKey: 'tour.searchAnalytics.title', descriptionKey: 'tour.searchAnalytics.desc', pageRoute: '/search-analytics', minRole: 'admin', stepCount: 3, group: 'chatSearch' },
  { tourId: 'lineage', labelKey: 'tour.lineage.title', descriptionKey: 'tour.lineage.desc', pageRoute: '/lineage', minRole: 'admin', stepCount: 3, group: 'chatSearch' },

  // Analytics & Quality
  { tourId: 'analytics', labelKey: 'tour.analytics.title', descriptionKey: 'tour.analytics.desc', pageRoute: '/analytics', minRole: 'admin', stepCount: 3, group: 'analytics' },
  { tourId: 'usage', labelKey: 'tour.usage.title', descriptionKey: 'tour.usage.desc', pageRoute: '/usage', minRole: 'admin', stepCount: 3, group: 'analytics' },
  { tourId: 'feedback', labelKey: 'tour.feedback.title', descriptionKey: 'tour.feedback.desc', pageRoute: '/feedback', minRole: 'admin', stepCount: 3, group: 'analytics' },
  { tourId: 'eval', labelKey: 'tour.eval.title', descriptionKey: 'tour.eval.desc', pageRoute: '/eval', minRole: 'super_admin', stepCount: 3, group: 'analytics' },
  { tourId: 'ab-tests', labelKey: 'tour.abTests.title', descriptionKey: 'tour.abTests.desc', pageRoute: '/ab-tests', minRole: 'super_admin', stepCount: 3, group: 'analytics' },

  // Access Control
  { tourId: 'users', labelKey: 'tour.users.title', descriptionKey: 'tour.users.desc', pageRoute: '/users', minRole: 'admin', stepCount: 4, group: 'access' },
  { tourId: 'permissions', labelKey: 'tour.permissions.title', descriptionKey: 'tour.permissions.desc', pageRoute: '/permissions', minRole: 'admin', stepCount: 3, group: 'access' },
  { tourId: 'tenants', labelKey: 'tour.tenants.title', descriptionKey: 'tour.tenants.desc', pageRoute: '/tenants', minRole: 'super_admin', stepCount: 4, group: 'access' },
  { tourId: 'roles', labelKey: 'tour.roles.title', descriptionKey: 'tour.roles.desc', pageRoute: '/roles', minRole: 'super_admin', stepCount: 3, group: 'access' },

  // System
  { tourId: 'settings', labelKey: 'tour.settings.title', descriptionKey: 'tour.settings.desc', pageRoute: '/settings', minRole: 'super_admin', stepCount: 5, group: 'system' },
  { tourId: 'connectors', labelKey: 'tour.connectors.title', descriptionKey: 'tour.connectors.desc', pageRoute: '/connectors', minRole: 'super_admin', stepCount: 3, group: 'system' },
  { tourId: 'backup', labelKey: 'tour.backup.title', descriptionKey: 'tour.backup.desc', pageRoute: '/backup', minRole: 'super_admin', stepCount: 3, group: 'system' },
  { tourId: 'audit-log', labelKey: 'tour.auditLog.title', descriptionKey: 'tour.auditLog.desc', pageRoute: '/audit-log', minRole: 'super_admin', stepCount: 3, group: 'system' },
];

export const GROUP_LABELS: Record<string, string> = {
  top: 'menu.dashboard',
  content: 'menu.group.content',
  chatSearch: 'menu.group.chatSearch',
  analytics: 'menu.group.analytics',
  access: 'menu.group.access',
  system: 'menu.group.system',
};
