import { api } from '../api.js';

export class DetectorComponent {
  constructor(app) {
    this.app = app;
    this.availableTypes = [];
    this.selectedRealtimeTypes = new Set();
    this.selectedManualTypes = new Set();

    this.enableSwitch = document.getElementById('detector-enable');
    this.confSlider = document.getElementById('min-confidence-slider');
    this.confVal = document.getElementById('min-confidence-val');
    this.realtimeChipsEl = document.getElementById('detector-classes-container');

    this.manualEnableSwitch = document.getElementById('manual-detection-enable');
    this.manualIntervalSlider = document.getElementById('manual-interval-slider');
    this.manualIntervalVal = document.getElementById('manual-interval-val');
    this.manualChipsEl = document.getElementById('manual-classes-container');

    this.initEvents();
  }

  initEvents() {
    // Slider live value feedback
    this.confSlider?.addEventListener('input', (e) => {
      const val = parseFloat(e.target.value);
      this.confVal.textContent = `${val.toFixed(2)} (${Math.round(val * 100)}%)`;
    });

    this.manualIntervalSlider?.addEventListener('input', (e) => {
      this.manualIntervalVal.textContent = `${e.target.value}s`;
    });

    // Class selection shortcuts
    document.getElementById('select-common-classes')?.addEventListener('click', () => {
      const common = ['person', 'car', 'motorcycle', 'bicycle', 'bus', 'truck', 'dog', 'cat'];
      this.selectedRealtimeTypes = new Set(this.availableTypes.filter(t => common.includes(t.toLowerCase())));
      this.renderChips();
    });

    document.getElementById('select-all-classes')?.addEventListener('click', () => {
      this.selectedRealtimeTypes = new Set(this.availableTypes);
      this.renderChips();
    });

    document.getElementById('clear-all-classes')?.addEventListener('click', () => {
      this.selectedRealtimeTypes.clear();
      this.renderChips();
    });

    // Save button
    document.getElementById('save-detector-btn')?.addEventListener('click', () => {
      this.saveSettings();
    });
  }

  async load() {
    try {
      const [config, types] = await Promise.all([
        api.getConfig(),
        api.getDetectorTypes(),
      ]);

      this.availableTypes = types || [];

      // Populate Real-time Detector settings
      if (config.detector) {
        this.enableSwitch.checked = Boolean(config.detector.enable);
        const conf = config.detector.min_confidence ?? 0.4;
        this.confSlider.value = conf;
        this.confVal.textContent = `${conf.toFixed(2)} (${Math.round(conf * 100)}%)`;

        const currentTypes = config.detector.types || [];
        this.selectedRealtimeTypes = new Set(currentTypes.map(t => typeof t === 'string' ? t : (t.type || t)));
      }

      // Populate Background Sequential Scanner settings
      if (config.manual_detection) {
        this.manualEnableSwitch.checked = Boolean(config.manual_detection.enable);
        const interval = config.manual_detection.interval_seconds ?? 30;
        this.manualIntervalSlider.value = interval;
        this.manualIntervalVal.textContent = `${interval}s`;

        const manualTypes = config.manual_detection.types || [];
        this.selectedManualTypes = new Set(manualTypes.map(t => typeof t === 'string' ? t : (t.type || t)));
      }

      this.renderChips();
    } catch (err) {
      console.error('Failed to load detector settings:', err);
      this.app.showToast(`Error loading detector settings: ${err.message}`, 'error');
    }
  }

  renderChips() {
    // Render Realtime Chips
    this.realtimeChipsEl.innerHTML = this.availableTypes.map(type => {
      const isActive = this.selectedRealtimeTypes.has(type);
      return `<button type="button" class="chip-btn ${isActive ? 'active' : ''}" data-type="${type}" aria-pressed="${isActive}">${type}</button>`;
    }).join('');

    this.realtimeChipsEl.querySelectorAll('.chip-btn').forEach(btn => {
      btn.addEventListener('click', () => {
        const type = btn.getAttribute('data-type');
        if (this.selectedRealtimeTypes.has(type)) {
          this.selectedRealtimeTypes.delete(type);
        } else {
          this.selectedRealtimeTypes.add(type);
        }
        this.renderChips();
      });
    });

    // Render Manual Detection Chips
    this.manualChipsEl.innerHTML = this.availableTypes.map(type => {
      const isActive = this.selectedManualTypes.has(type);
      return `<button type="button" class="chip-btn ${isActive ? 'active' : ''}" data-type="${type}" aria-pressed="${isActive}">${type}</button>`;
    }).join('');

    this.manualChipsEl.querySelectorAll('.chip-btn').forEach(btn => {
      btn.addEventListener('click', () => {
        const type = btn.getAttribute('data-type');
        if (this.selectedManualTypes.has(type)) {
          this.selectedManualTypes.delete(type);
        } else {
          this.selectedManualTypes.add(type);
        }
        this.renderChips();
      });
    });
  }

  async saveSettings() {
    try {
      this.app.showToast('Saving detector settings...', 'info');

      // Update detector config
      const detectorConfig = {
        enable: this.enableSwitch.checked,
        min_confidence: parseFloat(this.confSlider.value),
        types: Array.from(this.selectedRealtimeTypes),
      };

      // Update manual detection config
      const manualConfig = {
        enable: this.manualEnableSwitch.checked,
        interval_seconds: parseInt(this.manualIntervalSlider.value, 10),
        types: Array.from(this.selectedManualTypes),
      };

      await Promise.all([
        api.updateDetectorConfig(detectorConfig),
        api.updateManualDetectionConfig(manualConfig),
      ]);

      this.app.showToast('YOLO detection settings saved successfully!', 'success');
    } catch (err) {
      console.error('Failed to save detector settings:', err);
      this.app.showToast(`Error saving settings: ${err.message}`, 'error');
    }
  }
}
