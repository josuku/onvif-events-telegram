import { api } from '../api.js';

export class CamerasComponent {
  constructor(app) {
    this.app = app;
    this.cameras = [];
    this.activeCameraId = null;
    this.activeBlobUrl = null;

    this.gridEl = document.getElementById('cameras-grid');
    this.emptyEl = document.getElementById('cameras-empty');
    this.mediaDialog = document.getElementById('media-dialog');
    this.dialogImg = document.getElementById('dialog-snapshot-img');
    this.dialogCanvas = document.getElementById('dialog-detection-canvas');
    this.loadingOverlay = document.getElementById('media-loading');
    this.loadingText = document.getElementById('media-loading-text');
    this.resultsBar = document.getElementById('detection-results-bar');
    this.resultsSummary = document.getElementById('detected-objects-summary');
    this.dialogTitle = document.getElementById('media-dialog-title');
    this.dialogSubtitle = document.getElementById('media-dialog-subtitle');

    this.initEvents();
  }

  initEvents() {
    // Refresh all cameras button
    document.getElementById('refresh-cameras-btn')?.addEventListener('click', () => {
      this.loadCameras(true);
    });

    // Add camera button
    document.getElementById('add-camera-btn')?.addEventListener('click', () => {
      document.getElementById('add-camera-dialog')?.showModal();
    });

    // Add camera form
    document.getElementById('add-camera-form')?.addEventListener('submit', async (e) => {
      e.preventDefault();
      const uri = document.getElementById('new-camera-uri').value.trim();
      const user = document.getElementById('new-camera-user').value.trim();
      const pass = document.getElementById('new-camera-pass').value.trim();

      if (!uri) return;

      try {
        this.app.showToast('Connecting camera...', 'info');
        await api.addCamera(uri, user, pass);
        this.app.showToast('Camera added successfully!', 'success');
        document.getElementById('add-camera-dialog').close();
        document.getElementById('add-camera-form').reset();
        await this.loadCameras();
      } catch (err) {
        this.app.showToast(`Failed to add camera: ${err.message}`, 'error');
      }
    });

    // Dialog Refresh Snapshot button
    document.getElementById('dialog-refresh-snapshot-btn')?.addEventListener('click', () => {
      if (this.activeCameraId) {
        this.loadSnapshotInDialog(this.activeCameraId);
      }
    });

    // Dialog Run YOLO Detect button
    document.getElementById('dialog-run-detect-btn')?.addEventListener('click', () => {
      if (this.activeCameraId) {
        this.runDetectionInDialog(this.activeCameraId);
      }
    });

    // Clean up canvas/blob when dialog closes
    this.mediaDialog?.addEventListener('close', () => {
      if (this.activeBlobUrl) {
        URL.revokeObjectURL(this.activeBlobUrl);
        this.activeBlobUrl = null;
      }
      this.clearCanvas();
      this.resultsBar.classList.add('hidden');
    });
  }

  async loadCameras(refreshSnapshots = false) {
    try {
      this.cameras = await api.getCameras();
      this.renderGrid(refreshSnapshots);
    } catch (err) {
      console.error('Failed to load cameras:', err);
      this.app.showToast(`Error loading cameras: ${err.message}`, 'error');
    }
  }

  renderGrid(refreshSnapshots = false) {
    if (!this.cameras || this.cameras.length === 0) {
      this.gridEl.innerHTML = '';
      this.emptyEl.classList.remove('hidden');
      return;
    }

    this.emptyEl.classList.add('hidden');
    this.gridEl.innerHTML = this.cameras.map(cam => this.createCameraCardHtml(cam)).join('');

    // Attach card event listeners
    this.cameras.forEach(cam => {
      const card = this.gridEl.querySelector(`[data-camera-id="${cam.id}"]`);
      if (!card) return;

      // Quick snapshot buttons (both preview overlay and footer button)
      card.querySelectorAll('.btn-quick-snapshot').forEach(btn => {
        btn.addEventListener('click', (e) => {
          e.stopPropagation();
          this.openMediaDialog(cam.id, 'snapshot');
        });
      });

      // Quick YOLO detection buttons (both preview overlay and footer button)
      card.querySelectorAll('.btn-quick-detect').forEach(btn => {
        btn.addEventListener('click', (e) => {
          e.stopPropagation();
          this.openMediaDialog(cam.id, 'detect');
        });
      });

      // Delete camera button
      card.querySelector('.btn-delete-camera')?.addEventListener('click', (e) => {
        e.stopPropagation();
        this.confirmDeleteCamera(cam);
      });

      // Click on preview image opens dialog
      card.querySelector('.camera-preview-box')?.addEventListener('click', () => {
        this.openMediaDialog(cam.id, 'snapshot');
      });

      // Load initial snapshot thumbnail if available
      if (refreshSnapshots || !card.querySelector('.camera-preview-img')) {
        this.loadCardThumbnail(cam.id, card);
      }
    });
  }

  createCameraCardHtml(cam) {
    const statusText = cam.subscriptors_count > 0 ? 'Subscribed' : 'Ready';
    const statusClass = cam.subscriptors_count > 0 ? 'status-online' : 'status-online';
    const snapshotUrl = api.getSnapshotUrl(cam.id);

    return `
      <article class="camera-card" data-camera-id="${cam.id}">
        <header class="camera-card-header">
          <div class="camera-title-group">
            <span class="camera-name">${this.escapeHtml(cam.name)}</span>
            <span class="camera-address">${this.escapeHtml(cam.address || cam.uri)}</span>
          </div>
          <span class="status-pill ${statusClass}">
            <span class="status-dot"></span>
            ${statusText}
          </span>
        </header>

        <div class="camera-preview-box" title="Click to inspect snapshot">
          <img class="camera-preview-img" src="${snapshotUrl}" alt="${this.escapeHtml(cam.name)} snapshot" loading="lazy"
               onerror="this.style.display='none'; this.nextElementSibling.style.display='flex';">
          <div class="preview-placeholder" style="display: none;">
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M23 19a2 2 0 0 1-2 2H3a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h4l2-3h6l2 3h4a2 2 0 0 1 2 2z"></path><circle cx="12" cy="13" r="4"></circle></svg>
            <span>Click to load snapshot</span>
          </div>
          <div class="preview-overlay-actions">
            <button class="overlay-btn btn-quick-snapshot" title="Full Snapshot">
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M15 3h6v6M9 21H3v-6M21 3l-7 7M3 21l7-7"/></svg>
            </button>
            <button class="overlay-btn btn-quick-detect" title="YOLO Detection">
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><circle cx="12" cy="12" r="10"/><path d="m4.93 4.93 4.24 4.24"/><path d="m14.83 9.17 4.24-4.24"/><path d="m14.83 14.83 4.24 4.24"/><path d="m9.17 14.83-4.24 4.24"/></svg>
            </button>
          </div>
        </div>

        <div class="camera-card-body">
          <div class="meta-row">
            <span class="meta-label">Manufacturer</span>
            <span class="meta-value">${this.escapeHtml(cam.manufacturer || 'ONVIF')}</span>
          </div>
          <div class="meta-row">
            <span class="meta-label">Model</span>
            <span class="meta-value">${this.escapeHtml(cam.model || 'Generic')}</span>
          </div>
          <div class="meta-row">
            <span class="meta-label">Snapshot Method</span>
            <span class="meta-value">${this.escapeHtml(cam.snapshot_method?.toUpperCase() || 'API')}</span>
          </div>
        </div>

        <footer class="camera-card-footer">
          <button class="btn btn-secondary btn-quick-snapshot">
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M23 19a2 2 0 0 1-2 2H3a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h4l2-3h6l2 3h4a2 2 0 0 1 2 2z"/><circle cx="12" cy="13" r="4"/></svg>
            <span>Snapshot</span>
          </button>
          <button class="btn btn-primary btn-quick-detect">
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><circle cx="12" cy="12" r="10"/><path d="m4.93 4.93 4.24 4.24"/><path d="m14.83 9.17 4.24-4.24"/><path d="m14.83 14.83 4.24 4.24"/><path d="m9.17 14.83-4.24 4.24"/></svg>
            <span>Detect</span>
          </button>
          <button class="icon-button btn-delete-camera" title="Delete Camera">
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><polyline points="3 6 5 6 21 6"/><path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6m3 0V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2"/></svg>
          </button>
        </footer>
      </article>
    `;
  }

  loadCardThumbnail(cameraId, card) {
    const img = card.querySelector('.camera-preview-img');
    if (!img) return;
    const url = api.getSnapshotUrl(cameraId, Date.now());
    img.src = url;
  }

  async openMediaDialog(cameraId, initialMode = 'snapshot') {
    this.activeCameraId = cameraId;
    const camera = this.cameras.find(c => c.id === cameraId);

    this.dialogTitle.textContent = camera ? camera.name : `Camera ${cameraId}`;
    this.dialogSubtitle.textContent = camera ? camera.address || camera.uri : '';
    this.resultsBar.classList.add('hidden');
    this.clearCanvas();

    this.mediaDialog.showModal();

    if (initialMode === 'detect') {
      await this.runDetectionInDialog(cameraId);
    } else {
      await this.loadSnapshotInDialog(cameraId);
    }
  }

  async loadSnapshotInDialog(cameraId) {
    this.showLoading('Fetching snapshot...');
    this.clearCanvas();
    this.resultsBar.classList.add('hidden');

    try {
      const blob = await api.fetchSnapshotBlob(cameraId);
      if (this.activeBlobUrl) {
        URL.revokeObjectURL(this.activeBlobUrl);
      }
      this.activeBlobUrl = URL.createObjectURL(blob);
      this.dialogImg.src = this.activeBlobUrl;

      await new Promise((resolve) => {
        this.dialogImg.onload = resolve;
      });

      document.getElementById('snapshot-timestamp').textContent = `Updated: ${new Date().toLocaleTimeString()}`;
    } catch (err) {
      this.app.showToast(`Snapshot error: ${err.message}`, 'error');
    } finally {
      this.hideLoading();
    }
  }

  async runDetectionInDialog(cameraId) {
    // Ensure we have a fresh snapshot
    await this.loadSnapshotInDialog(cameraId);

    this.showLoading('Running YOLO detection...');
    try {
      const res = await api.detectObjects(cameraId);
      this.drawDetections(res.objects || []);
      this.displayDetectionSummary(res.objects || []);
    } catch (err) {
      this.app.showToast(`Detection error: ${err.message}`, 'error');
    } finally {
      this.hideLoading();
    }
  }

  drawDetections(objects) {
    const img = this.dialogImg;
    const canvas = this.dialogCanvas;
    if (!img.complete || img.naturalWidth === 0) return;

    // Match canvas coordinate size to image natural size
    canvas.width = img.naturalWidth;
    canvas.height = img.naturalHeight;

    const ctx = canvas.getContext('2d');
    ctx.clearRect(0, 0, canvas.width, canvas.height);

    if (!objects || objects.length === 0) return;

    objects.forEach(obj => {
      const x = obj.x1;
      const y = obj.y1;
      const width = obj.x2 - obj.x1;
      const height = obj.y2 - obj.y1;

      // Box styling
      ctx.strokeStyle = '#22c55e'; // Vibrant green
      ctx.lineWidth = Math.max(3, Math.round(canvas.width / 400));
      ctx.strokeRect(x, y, width, height);

      // Label background & text
      const confidencePercent = Math.round(obj.confidence * 100);
      const label = `${obj.class} ${confidencePercent}%`;

      const fontSize = Math.max(14, Math.round(canvas.width / 50));
      ctx.font = `bold ${fontSize}px sans-serif`;
      const textMetrics = ctx.measureText(label);
      const padding = 6;
      const bgWidth = textMetrics.width + padding * 2;
      const bgHeight = fontSize + padding * 1.5;

      ctx.fillStyle = 'rgba(22, 163, 74, 0.9)';
      ctx.fillRect(x, Math.max(0, y - bgHeight), bgWidth, bgHeight);

      ctx.fillStyle = '#ffffff';
      ctx.fillText(label, x + padding, Math.max(fontSize, y - padding));
    });
  }

  displayDetectionSummary(objects) {
    this.resultsBar.classList.remove('hidden');
    if (!objects || objects.length === 0) {
      this.resultsSummary.textContent = 'No target objects detected above confidence threshold.';
      return;
    }

    const counts = {};
    objects.forEach(o => {
      counts[o.class] = (counts[o.class] || 0) + 1;
    });

    const summaryParts = Object.entries(counts).map(([cls, count]) => `${count} ${cls}${count > 1 ? 's' : ''}`);
    this.resultsSummary.textContent = summaryParts.join(', ');
  }

  clearCanvas() {
    const ctx = this.dialogCanvas?.getContext('2d');
    if (ctx) {
      ctx.clearRect(0, 0, this.dialogCanvas.width, this.dialogCanvas.height);
    }
  }

  showLoading(text = 'Loading...') {
    this.loadingText.textContent = text;
    this.loadingOverlay.classList.remove('hidden');
  }

  hideLoading() {
    this.loadingOverlay.classList.add('hidden');
  }

  confirmDeleteCamera(cam) {
    this.app.confirmAction(
      'Delete Camera',
      `Are you sure you want to remove "${cam.name}" (${cam.address || cam.uri})?`,
      async () => {
        try {
          await api.deleteCamera(cam.id);
          this.app.showToast('Camera deleted successfully', 'success');
          await this.loadCameras();
        } catch (err) {
          this.app.showToast(`Delete failed: ${err.message}`, 'error');
        }
      }
    );
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
