import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { deleteUser, listUsers } from '../api/users';

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
