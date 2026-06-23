import { request } from './_base';

export interface AuthLoginResponse {
  token: string;
  user_id: number;
  username: string;
  mfa_required?: boolean;
}

export const auth = {
  register: (username: string, email: string, password: string) =>
    request<{ id: number; username: string }>('/users/register', {
      method: 'POST',
      body: JSON.stringify({ username, email, password }),
    }),
  login: (username: string, password: string) =>
    request<AuthLoginResponse>('/users/login', {
      method: 'POST',
      body: JSON.stringify({ username, password }),
    }),
  verifyMfa: (username: string, code: string, backup = false) =>
    request<AuthLoginResponse>('/users/mfa/verify', {
      method: 'POST',
      body: JSON.stringify({ username, code, backup }),
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
