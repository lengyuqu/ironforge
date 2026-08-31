// Auth state store using Svelte 5 runes.
//
// Auth model: HttpOnly Cookie 单轨制.  Backend sets the `ironforge_token` cookie
// on every successful auth flow (login / MFA / SSO / reset-password).  We no
// longer hold a copy of the JWT in JS — `fetchUser()` is the single source
// of truth.  Logout clears both server-side cookie and our local state.

import { auth } from '$lib/api/client.svelte';
import type { User } from '$lib/types/entities';
import { toErrorMessage } from '$lib/utils/error';

let currentUser = $state<User | null>(null);
let isLoading = $state(false);
let error = $state<string | null>(null);
let authReady = $state(false); // True after initial fetchUser() completes
let pendingMfaUsername = $state<string | null>(null);

export function getUser() {
  return currentUser;
}

export function isLoggedIn() {
  return currentUser !== null;
}

export function isAdmin() {
  return currentUser?.is_admin === true;
}

export function getAuthError() {
  return error;
}

export function getAuthLoading() {
  return isLoading;
}

export function isMfaRequired() {
  return pendingMfaUsername !== null;
}

export function beginMfa(username: string) {
  pendingMfaUsername = username;
  error = null;
}

export function isAuthReady() {
  return authReady;
}

export async function login(username: string, password: string) {
  isLoading = true;
  error = null;
  try {
    const res = await auth.login(username, password);
    if (res.mfa_required) {
      currentUser = null;
      pendingMfaUsername = res.username || username;
      return false;
    }

    pendingMfaUsername = null;
    // Backend set-cookie on success; now fetch the full profile.
    const me = await auth.me();
    currentUser = {
      id: me.id,
      username: me.username,
      email: me.email,
      is_admin: me.is_admin ?? false,
      display_name: me.display_name,
    };
    return true;
  } catch (e) {
    error = toErrorMessage(e, 'Login failed');
    return false;
  } finally {
    isLoading = false;
  }
}

export async function verifyMfa(code: string, backup = false) {
  if (!pendingMfaUsername) {
    error = 'MFA verification is not pending';
    return false;
  }

  isLoading = true;
  error = null;
  try {
    await auth.verifyMfa(pendingMfaUsername, code, backup);
    pendingMfaUsername = null;
    // Backend set-cookie on success; fetch full profile.
    const me = await auth.me();
    currentUser = {
      id: me.id,
      username: me.username,
      email: me.email,
      is_admin: me.is_admin ?? false,
      display_name: me.display_name,
    };
    return true;
  } catch (e) {
    error = toErrorMessage(e, 'MFA verification failed');
    return false;
  } finally {
    isLoading = false;
  }
}

export async function register(username: string, email: string, password: string) {
  isLoading = true;
  error = null;
  try {
    await auth.register(username, email, password);
    // Auto login after register — backend sets cookie on login.
    return await login(username, password);
  } catch (e) {
    error = toErrorMessage(e, 'Registration failed');
    return false;
  } finally {
    isLoading = false;
  }
}

export async function fetchUser() {
  // Relies purely on HttpOnly cookie — the backend validates it and returns
  // 401 if absent/invalid.  No Bearer header is attached.
  try {
    const me = await auth.me();
    currentUser = {
      id: me.id,
      username: me.username,
      email: me.email,
      is_admin: me.is_admin ?? false,
      display_name: me.display_name,
    };
  } catch {
    currentUser = null;
  } finally {
    authReady = true;
  }
}

export async function logout() {
  // Ask the backend to clear the HttpOnly cookie (JS can't read/delete it).
  try {
    await auth.logout();
  } catch {
    // Ignore — cookie may already be cleared or user already logged out.
  }
  currentUser = null;
  pendingMfaUsername = null;
}
