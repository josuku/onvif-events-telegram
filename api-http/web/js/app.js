import { api } from './api.js';
import { CamerasComponent } from './components/cameras.js';
import { DetectorComponent } from './components/detector.js';
import { RecordingsComponent } from './components/recordings.js';
import { SettingsComponent } from './components/settings.js';

class App {
  constructor() {
    this.currentTab = 'cameras';
    this.confirmCallback = null;

    // Components
    this.camerasComp = new CamerasComponent(this);
    this.detectorComp = new DetectorComponent(this);
    this.recordingsComp = new RecordingsComponent(this);
    this.settingsComp = new SettingsComponent(this);

    this.initTheme();
    this.initNavigation();
    this.initAuth();
    this.initConfirmation();

    // Boot
    this.start();
  }

  // --- Theme Management ---
  initTheme() {
    const savedTheme = localStorage.getItem('oet_theme');
    if (savedTheme) {
      document.documentElement.setAttribute('data-theme', savedTheme);
    }

    document.getElementById('theme-toggle')?.addEventListener('click', () => {
      const current = document.documentElement.getAttribute('data-theme');
      const isDark = current === 'dark' || (!current && window.matchMedia('(prefers-color-scheme: dark)').matches);
      const newTheme = isDark ? 'light' : 'dark';
      document.documentElement.setAttribute('data-theme', newTheme);
      localStorage.setItem('oet_theme', newTheme);
    });
  }

  // --- Navigation & Routing ---
  initNavigation() {
    const tabs = document.querySelectorAll('.nav-tab');
    tabs.forEach(tab => {
      tab.addEventListener('click', () => {
        const target = tab.getAttribute('data-tab');
        this.switchTab(target);
      });
    });

    window.addEventListener('hashchange', () => {
      const hash = window.location.hash.replace('#', '');
      if (hash && ['cameras', 'detector', 'recordings', 'settings'].includes(hash)) {
        this.switchTab(hash, false);
      }
    });
  }

  switchTab(tabId, updateHash = true) {
    if (updateHash) {
      window.location.hash = `#${tabId}`;
    }
    this.currentTab = tabId;

    // Update tab buttons
    document.querySelectorAll('.nav-tab').forEach(btn => {
      const match = btn.getAttribute('data-tab') === tabId;
      btn.classList.toggle('active', match);
      btn.setAttribute('aria-selected', match ? 'true' : 'false');
    });

    // Update panels
    document.querySelectorAll('.tab-panel').forEach(panel => {
      const match = panel.id === `tab-${tabId}`;
      panel.classList.toggle('active', match);
    });

    // Lazy load data for selected tab
    if (tabId === 'cameras') {
      this.camerasComp.loadCameras();
    } else if (tabId === 'detector') {
      this.detectorComp.load();
    } else if (tabId === 'recordings') {
      this.recordingsComp.load(this.camerasComp.cameras);
    } else if (tabId === 'settings') {
      this.settingsComp.load();
    }
  }

  // --- Authentication ---
  initAuth() {
    const loginDialog = document.getElementById('login-dialog');
    const loginForm = document.getElementById('login-form');
    const loginError = document.getElementById('login-error');
    const logoutBtn = document.getElementById('logout-btn');
    const userBadge = document.getElementById('user-badge');

    userBadge.textContent = api.getUser();

    // Listen for auth failure events from ApiClient
    window.addEventListener('auth:required', () => {
      loginError.classList.add('hidden');
      loginDialog.showModal();
    });

    loginForm?.addEventListener('submit', async (e) => {
      e.preventDefault();
      loginError.classList.add('hidden');
      const user = document.getElementById('login-username').value.trim();
      const pass = document.getElementById('login-password').value.trim();

      try {
        await api.login(user, pass);
        userBadge.textContent = user;
        loginDialog.close();
        loginForm.reset();
        this.showToast(`Welcome back, ${user}!`, 'success');
        this.start();
      } catch (err) {
        loginError.textContent = err.message || 'Invalid username or password';
        loginError.classList.remove('hidden');
      }
    });

    logoutBtn?.addEventListener('click', async () => {
      await api.logout();
      this.showToast('Logged out successfully', 'info');
      loginDialog.showModal();
    });
  }

  // --- Confirmation Dialog ---
  initConfirmation() {
    const dialog = document.getElementById('confirm-dialog');
    const okBtn = document.getElementById('confirm-ok-btn');
    const cancelBtn = document.getElementById('confirm-cancel-btn');

    cancelBtn?.addEventListener('click', () => {
      dialog.close();
      this.confirmCallback = null;
    });

    okBtn?.addEventListener('click', () => {
      if (this.confirmCallback) {
        const cb = this.confirmCallback;
        this.confirmCallback = null;
        cb();
      }
      dialog.close();
    });
  }

  confirmAction(title, message, onConfirm) {
    document.getElementById('confirm-dialog-title').textContent = title;
    document.getElementById('confirm-dialog-message').textContent = message;
    this.confirmCallback = onConfirm;
    document.getElementById('confirm-dialog').showModal();
  }

  // --- Toast Notifications ---
  showToast(message, type = 'info') {
    const container = document.getElementById('toast-container');
    if (!container) return;

    const toast = document.createElement('div');
    toast.className = `toast toast-${type}`;
    toast.innerHTML = `<span>${this.escapeHtml(message)}</span>`;

    container.appendChild(toast);
    setTimeout(() => {
      toast.style.opacity = '0';
      toast.style.transform = 'translateY(10px)';
      toast.style.transition = 'all 200ms ease';
      setTimeout(() => toast.remove(), 250);
    }, 3500);
  }

  escapeHtml(str) {
    if (!str) return '';
    return str.replace(/[&<>'"]/g, tag => ({
      '&': '&amp;',
      '<': '&lt;',
      '>': '&gt;',
      "'": '&#39;',
      '"': '&quot;',
    }[tag] || tag));
  }

  // --- Start Up ---
  async start() {
    const hash = window.location.hash.replace('#', '') || 'cameras';

    try {
      // Test connectivity / auth
      await api.checkMe();
      document.getElementById('connection-status').className = 'status-pill status-online';
      document.querySelector('#connection-status .status-text').textContent = 'Connected';
    } catch (_) {
      // 401 is intercepted by ApiClient which dispatches auth:required
      return;
    }

    this.switchTab(hash);
  }
}

// Instantiate on DOM load
window.addEventListener('DOMContentLoaded', () => {
  window.app = new App();
});
