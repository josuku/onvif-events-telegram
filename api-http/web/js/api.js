/**
 * Modern API Client for ONVIF Events & YOLO Backend
 * Supports JWT authentication, Bearer tokens, and reactive error interception.
 */

class ApiClient {
  constructor() {
    this.tokenKey = 'oet_auth_token';
    this.userKey = 'oet_auth_user';
  }

  getToken() {
    return localStorage.getItem(this.tokenKey);
  }

  setToken(token, user = 'admin') {
    if (token) {
      localStorage.setItem(this.tokenKey, token);
      localStorage.setItem(this.userKey, user);
    } else {
      localStorage.removeItem(this.tokenKey);
      localStorage.removeItem(this.userKey);
    }
  }

  getUser() {
    return localStorage.getItem(this.userKey) || 'Admin';
  }

  isAuthenticated() {
    return Boolean(this.getToken());
  }

  async request(path, options = {}) {
    const headers = new Headers(options.headers || {});
    const token = this.getToken();

    if (token && !headers.has('Authorization')) {
      headers.set('Authorization', `Bearer ${token}`);
    }

    if (options.body && typeof options.body === 'object' && !(options.body instanceof FormData)) {
      headers.set('Content-Type', 'application/json');
      options.body = JSON.stringify(options.body);
    }

    options.headers = headers;

    try {
      const response = await fetch(path, options);

      if (response.status === 401) {
        this.setToken(null);
        window.dispatchEvent(new CustomEvent('auth:required'));
        throw new Error('Session expired or unauthorized. Please sign in.');
      }

      if (!response.ok) {
        let errorMsg = `HTTP Error ${response.status}`;
        try {
          const errJson = await response.json();
          errorMsg = errJson.message || errJson.error || errorMsg;
        } catch (_) {
          const text = await response.text();
          if (text) errorMsg = text;
        }
        throw new Error(errorMsg);
      }

      // Check if response is json or raw blob
      const contentType = response.headers.get('content-type') || '';
      if (contentType.includes('application/json')) {
        return await response.json();
      } else if (contentType.includes('image/') || contentType.includes('video/')) {
        return await response.blob();
      }

      return response;
    } catch (err) {
      console.error(`[API] Error on ${options.method || 'GET'} ${path}:`, err);
      throw err;
    }
  }

  // --- Auth APIs ---
  async login(username, password) {
    const res = await this.request('/api/v1/auth/login', {
      method: 'POST',
      body: { username, password },
    });
    if (res && res.token) {
      this.setToken(res.token, username);
    }
    return res;
  }

  async logout() {
    try {
      await this.request('/api/v1/auth/logout', { method: 'POST' });
    } catch (_) {}
    this.setToken(null);
  }

  async checkMe() {
    return await this.request('/api/v1/auth/me');
  }

  // --- Camera APIs ---
  async getCameras() {
    return await this.request('/api/v1/cameras');
  }

  async getCamera(id) {
    return await this.request(`/api/v1/cameras/${id}`);
  }

  async addCamera(uri, username, password) {
    return await this.request('/api/v1/cameras', {
      method: 'POST',
      body: { uri, username, password },
    });
  }

  async deleteCamera(id) {
    return await this.request(`/api/v1/cameras/${id}`, {
      method: 'DELETE',
    });
  }

  async setCameraName(id, name) {
    return await this.request(`/api/v1/cameras/${id}/name`, {
      method: 'PUT',
      body: { name },
    });
  }

  // --- Media & Snapshot APIs ---
  getSnapshotUrl(id, t = Date.now()) {
    const token = this.getToken();
    const query = new URLSearchParams({ t });
    if (token) query.set('token', token);
    return `/api/v1/cameras/${id}/snapshot?${query.toString()}`;
  }

  async fetchSnapshotBlob(id) {
    return await this.request(`/api/v1/cameras/${id}/snapshot?t=${Date.now()}`);
  }

  async detectObjects(id) {
    return await this.request(`/api/v1/cameras/${id}/detect`, {
      method: 'POST',
    });
  }

  // --- Recordings APIs ---
  async getRecordingsSummary(id, days = 7) {
    return await this.request(`/api/v1/cameras/${id}/recordings/summary?days=${days}`);
  }

  async getRecordingsList(id, date, clipSeconds = 30) {
    const params = new URLSearchParams();
    if (date) params.set('date', date);
    if (clipSeconds) params.set('clip_seconds', clipSeconds);
    return await this.request(`/api/v1/cameras/${id}/recordings?${params.toString()}`);
  }

  getRecordingDownloadUrl(id, date, clipSeconds = 30) {
    const token = this.getToken();
    const params = new URLSearchParams({ date, clip_seconds: clipSeconds });
    if (token) params.set('token', token);
    return `/api/v1/cameras/${id}/recordings/download?${params.toString()}`;
  }

  // --- Configuration APIs ---
  async getConfig() {
    return await this.request('/api/v1/config');
  }

  async resetConfig() {
    return await this.request('/api/v1/config/reset', {
      method: 'POST',
    });
  }

  async getDetectorTypes() {
    return await this.request('/api/v1/config/detector-types');
  }

  async updateDetectorConfig(config) {
    return await this.request('/api/v1/config/detector', {
      method: 'PATCH',
      body: config,
    });
  }

  async updateManualDetectionConfig(config) {
    return await this.request('/api/v1/config/manual-detection', {
      method: 'PATCH',
      body: config,
    });
  }

  async updatePollingConfig(config) {
    return await this.request('/api/v1/config/polling', {
      method: 'PATCH',
      body: config,
    });
  }
}

export const api = new ApiClient();
