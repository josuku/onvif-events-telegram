import { api } from '../api.js';

export class RecordingsComponent {
  constructor(app) {
    this.app = app;
    this.cameras = [];

    this.cameraSelect = document.getElementById('recording-camera-select');
    this.dateInput = document.getElementById('recording-date-input');
    this.clipInput = document.getElementById('recording-clip-input');
    this.summaryBanner = document.getElementById('recording-summary-container');
    this.tableBody = document.getElementById('recordings-table-body');

    this.totalFilesEl = document.getElementById('summary-total-files');
    this.totalSizeEl = document.getElementById('summary-total-size');
    this.firstBeginEl = document.getElementById('summary-first-begin');
    this.lastEndEl = document.getElementById('summary-last-end');

    this.initEvents();
  }

  initEvents() {
    // Set default date to today
    const today = new Date().toISOString().split('T')[0];
    this.dateInput.value = today;

    // Presets row
    document.querySelectorAll('.preset-pill').forEach(btn => {
      btn.addEventListener('click', () => {
        const preset = btn.getAttribute('data-preset');
        this.applyDatePreset(preset);
      });
    });

    // Query list button
    document.getElementById('query-recordings-btn')?.addEventListener('click', () => {
      this.searchRecordings();
    });

    // Query summary button
    document.getElementById('query-summary-btn')?.addEventListener('click', () => {
      this.fetchSummary();
    });
  }

  async load(cameras = null) {
    try {
      if (!cameras) {
        this.cameras = await api.getCameras();
      } else {
        this.cameras = cameras;
      }

      this.populateCameraSelect();
    } catch (err) {
      console.error('Failed to load recordings component:', err);
    }
  }

  populateCameraSelect() {
    const currentVal = this.cameraSelect.value;
    this.cameraSelect.innerHTML = '<option value="">Select a camera...</option>' +
      this.cameras.map(c => `<option value="${c.id}">${this.escapeHtml(c.name)} (${c.address || c.uri})</option>`).join('');

    if (currentVal && this.cameras.some(c => String(c.id) === currentVal)) {
      this.cameraSelect.value = currentVal;
    } else if (this.cameras.length > 0) {
      this.cameraSelect.value = this.cameras[0].id;
    }
  }

  applyDatePreset(preset) {
    const now = new Date();
    let target = new Date();

    if (preset === 'today') {
      target = now;
    } else if (preset === 'yesterday') {
      target.setDate(now.getDate() - 1);
    } else if (preset === '3days') {
      target.setDate(now.getDate() - 3);
    } else if (preset === '7days') {
      target.setDate(now.getDate() - 7);
    }

    this.dateInput.value = target.toISOString().split('T')[0];
    this.searchRecordings();
  }

  async searchRecordings() {
    const cameraId = this.cameraSelect.value;
    if (!cameraId) {
      this.app.showToast('Please select a camera first', 'warning');
      return;
    }

    const dateVal = this.dateInput.value;
    const clipSeconds = parseInt(this.clipInput.value, 10) || 30;

    this.tableBody.innerHTML = `
      <tr>
        <td colspan="6" class="text-center text-muted">
          <div style="display:flex; justify-content:center; align-items:center; gap:0.5rem; padding:1.5rem;">
            <div class="spinner" style="width:1.5rem; height:1.5rem;"></div>
            <span>Searching camera recordings...</span>
          </div>
        </td>
      </tr>
    `;

    try {
      const recordings = await api.getRecordingsList(cameraId, dateVal, clipSeconds);
      this.renderTable(recordings, cameraId, dateVal, clipSeconds);

      // Also trigger a summary query in background to keep stats banner updated
      this.fetchSummary(false);
    } catch (err) {
      console.error('Failed to search recordings:', err);
      this.tableBody.innerHTML = `
        <tr>
          <td colspan="6" class="text-center" style="color:var(--danger); padding:1.5rem;">
            Error querying recordings: ${this.escapeHtml(err.message)}
          </td>
        </tr>
      `;
    }
  }

  async fetchSummary(showToast = true) {
    const cameraId = this.cameraSelect.value;
    if (!cameraId) {
      if (showToast) this.app.showToast('Please select a camera first', 'warning');
      return;
    }

    try {
      const summary = await api.getRecordingsSummary(cameraId, 7);
      this.renderSummary(summary);
      if (showToast) this.app.showToast('Summary statistics updated', 'info');
    } catch (err) {
      console.error('Failed to fetch recording summary:', err);
      if (showToast) this.app.showToast(`Summary error: ${err.message}`, 'error');
    }
  }

  renderSummary(summary) {
    if (!summary) return;
    this.summaryBanner.classList.remove('hidden');

    this.totalFilesEl.textContent = summary.total_files ?? 0;
    const size = summary.total_size_mb || 0;
    this.totalSizeEl.textContent = size >= 1024 ? `${(size / 1024).toFixed(2)} GB` : `${size.toFixed(1)} MB`;
    this.firstBeginEl.textContent = summary.first_begin ? this.formatDate(summary.first_begin) : 'None';
    this.lastEndEl.textContent = summary.last_end ? this.formatDate(summary.last_end) : 'None';
  }

  renderTable(recordings, cameraId, dateVal, clipSeconds) {
    if (!recordings || recordings.length === 0) {
      this.tableBody.innerHTML = `
        <tr>
          <td colspan="6" class="text-center text-muted" style="padding:2rem;">
            No recording files found for ${this.escapeHtml(dateVal || 'this date')}.
          </td>
        </tr>
      `;
      return;
    }

    this.tableBody.innerHTML = recordings.map((rec, idx) => {
      const durationSec = this.calculateDuration(rec.begin, rec.end);
      const downloadUrl = api.getRecordingDownloadUrl(cameraId, rec.begin, clipSeconds);

      return `
        <tr>
          <td><strong>${this.escapeHtml(rec.name || `File #${idx + 1}`)}</strong></td>
          <td>${this.escapeHtml(rec.begin)}</td>
          <td>${this.escapeHtml(rec.end)}</td>
          <td>${durationSec}</td>
          <td>${rec.size_mb ? `${rec.size_mb.toFixed(1)} MB` : '—'}</td>
          <td>
            <a href="${downloadUrl}" class="btn btn-secondary" style="padding:0.25rem 0.625rem; font-size:0.75rem;" download>
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" style="width:0.875rem; height:0.875rem;"><path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"/><polyline points="7 10 12 15 17 10"/><line x1="12" y1="15" x2="12" y2="3"/></svg>
              <span>Download</span>
            </a>
          </td>
        </tr>
      `;
    }).join('');
  }

  calculateDuration(beginStr, endStr) {
    if (!beginStr || !endStr) return '—';
    try {
      const b = new Date(beginStr).getTime();
      const e = new Date(endStr).getTime();
      if (isNaN(b) || isNaN(e)) return '—';
      const diffSec = Math.max(0, Math.round((e - b) / 1000));
      const m = Math.floor(diffSec / 60);
      const s = diffSec % 60;
      return m > 0 ? `${m}m ${s}s` : `${s}s`;
    } catch (_) {
      return '—';
    }
  }

  formatDate(dateStr) {
    try {
      const d = new Date(dateStr);
      return isNaN(d.getTime()) ? dateStr : d.toLocaleString();
    } catch (_) {
      return dateStr;
    }
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
}
