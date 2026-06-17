import { request } from './_base';

export const auth = {
  register: (username: string, email: string, password: string) =>
    request<{ id: number; username: string }>('/users/register', {
      method: 'POST',
      body: JSON.stringify({ username, email, password }),
    }),
  login: (username: string, password: string) =>
    request<{ token: string; user: { id: number; username: string; email: string } }>('/users/login', {
      method: 'POST',
      body: JSON.stringify({ username, password }),
    }),
  me: () =>
    request<{ id: number; username: string; email: string; is_admin: boolean; display_name: string | null }>('/users/me'),
  forgotPassword: (email: string) =>
    request<{ message: string }>('/users/forgot-password', {
      method: 'POST',
      body: JSON.stringify({ email }),
    }),
  resetPassword: (token: string, newPassword: string) =>
    request<{ token: string; user_id: number; username: string }>('/users/reset-password', {
      method: 'POST',
      body: JSON.stringify({ token, new_password: newPassword }),
    }),
};
