import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { deleteUser, listUsers, updateUserRole, updateUserStatus } from '../api/users';
import type { UserRole } from '../api/types';

export function useUsers() {
  return useQuery({ queryKey: ['users'], queryFn: () => listUsers() });
}

export function useDeleteUser() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (id: string) => deleteUser(id),
    onSuccess: () => qc.invalidateQueries({ queryKey: ['users'] }),
  });
}

export function useUpdateUserRole() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: ({ id, role }: { id: string; role: UserRole }) => updateUserRole(id, role),
    onSuccess: () => qc.invalidateQueries({ queryKey: ['users'] }),
  });
}

export function useUpdateUserStatus() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: ({ id, disabled }: { id: string; disabled: boolean }) =>
      updateUserStatus(id, disabled),
    onSuccess: () => qc.invalidateQueries({ queryKey: ['users'] }),
  });
}
