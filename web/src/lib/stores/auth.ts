// Auth state store using Svelte 5 runes

import { setToken, getToken, auth } from '$lib/api/client';

interface User {
  id: number;
  username: string;
  email: string;
}

let currentUser = $state<User | null>(null);
let isLoading = $state(false);
let error = $state<string | null>(null);

export function getUser() {
  return currentUser;
}

export function isLoggedIn() {
  return currentUser !== null;
}

export function getAuthError() {
  return error;
}

export function getAuthLoading() {
  return isLoading;
}

export async function login(username: string, password: string) {
  isLoading = true;
  error = null;
  try {
    const res = await auth.login(username, password);
    setToken(res.token);
    currentUser = res.user;
    return true;
  } catch (e: any) {
    error = e.message || 'Login failed';
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
    // Auto login after register
    return await login(username, password);
  } catch (e: any) {
    error = e.message || 'Registration failed';
    return false;
  } finally {
    isLoading = false;
  }
}

export async function fetchUser() {
  const token = getToken();
  if (!token) {
    currentUser = null;
    return;
  }
  try {
    currentUser = await auth.me();
  } catch {
    setToken(null);
    currentUser = null;
  }
}

export function logout() {
  setToken(null);
  currentUser = null;
}
