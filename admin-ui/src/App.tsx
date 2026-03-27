import { BrowserRouter, Routes, Route } from 'react-router-dom';
import { AdminLayout } from './layouts/AdminLayout';
import { AuthLayout } from './layouts/AuthLayout';
import { ProtectedRoute } from './auth/ProtectedRoute';
import { RoleRoute } from './auth/SuperAdminRoute';
import { LoginPage } from './pages/LoginPage';
import { DashboardPage } from './pages/DashboardPage';
import { KmPage } from './pages/KmPage';
import { DocumentsPage } from './pages/DocumentsPage';
import { UsersPage } from './pages/UsersPage';
import { PermissionsPage } from './pages/PermissionsPage';
import { HealthPage } from './pages/HealthPage';
import { SettingsPage } from './pages/SettingsPage';
import { TestChatPage } from './pages/TestChatPage';
import { UsagePage } from './pages/UsagePage';
import { FeedbackPage } from './pages/FeedbackPage';
import { ConnectorsPage } from './pages/ConnectorsPage';
import InferenceLogsPage from './pages/InferenceLogsPage';
import AnalyticsPage from './pages/AnalyticsPage';
import EvalPage from './pages/EvalPage';

export function App() {
  return (
    <BrowserRouter>
      <Routes>
        <Route element={<AuthLayout />}>
          <Route path="/login" element={<LoginPage />} />
        </Route>
        <Route
          element={
            <ProtectedRoute>
              <AdminLayout />
            </ProtectedRoute>
          }
        >
          <Route path="/" element={<DashboardPage />} />
          <Route path="/km" element={<RoleRoute minRole="editor"><KmPage /></RoleRoute>} />
          <Route path="/documents" element={<RoleRoute minRole="editor"><DocumentsPage /></RoleRoute>} />
          <Route path="/test-chat" element={<RoleRoute minRole="editor"><TestChatPage /></RoleRoute>} />
          <Route path="/users" element={<RoleRoute minRole="admin"><UsersPage /></RoleRoute>} />
          <Route path="/permissions" element={<RoleRoute minRole="admin"><PermissionsPage /></RoleRoute>} />
          <Route path="/usage" element={<RoleRoute minRole="admin"><UsagePage /></RoleRoute>} />
          <Route path="/feedback" element={<RoleRoute minRole="admin"><FeedbackPage /></RoleRoute>} />
          <Route path="/connectors" element={<RoleRoute minRole="super_admin"><ConnectorsPage /></RoleRoute>} />
          <Route path="/analytics" element={<RoleRoute minRole="admin"><AnalyticsPage /></RoleRoute>} />
          <Route path="/inference-logs" element={<RoleRoute minRole="super_admin"><InferenceLogsPage /></RoleRoute>} />
          <Route path="/eval" element={<RoleRoute minRole="super_admin"><EvalPage /></RoleRoute>} />
          <Route path="/settings" element={<RoleRoute minRole="super_admin"><SettingsPage /></RoleRoute>} />
          <Route path="/system" element={<HealthPage />} />
        </Route>
      </Routes>
    </BrowserRouter>
  );
}
