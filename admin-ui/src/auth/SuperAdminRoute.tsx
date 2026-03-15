import { Navigate } from 'react-router-dom';
import { useAuth } from './useAuth';
import type { UserRole } from '../api/types';

const ROLE_LEVEL: Record<UserRole, number> = {
  super_admin: 4,
  admin: 3,
  editor: 2,
  viewer: 1,
};

/** Route guard that requires a minimum role. Defaults to super_admin. */
export function SuperAdminRoute({ children }: { children: React.ReactNode }) {
  return <RoleRoute minRole="super_admin">{children}</RoleRoute>;
}

export function RoleRoute({
  children,
  minRole,
}: {
  children: React.ReactNode;
  minRole: UserRole;
}) {
  const { user } = useAuth();
  const userLevel = ROLE_LEVEL[user?.role ?? 'viewer'] ?? 1;
  const requiredLevel = ROLE_LEVEL[minRole] ?? 1;
  if (userLevel < requiredLevel) return <Navigate to="/" replace />;
  return <>{children}</>;
}
