import { api } from '../api.js';

export class SettingsComponent {
  constructor(app) {
    this.app = app;

    this.pollingSecondsInput = document.getElementById('cfg-polling-seconds');
    this.betweenSecondsInput = document.getElementById('cfg-between-seconds');
    this.recordingClipInput = document.getElementById('cfg-recording-clip');
    this.sendErrorsSwitch = document.getElementById('cfg-send-errors');
    this.autoRenewalSwitch = document.getElementById('cfg-auto-renewal');

    this.initEvents();
  }

  initEvents() {
    // Save Settings button
    document.getElementById('save-general-settings-btn')?.addEventListener('click', () => {
      this.saveSettings();
    });

    // Reset Defaults button
    document.getElementById('reset-config-btn')?.addEventListener('click', () => {
      this.app.confirmAction(
        'Reset Configuration',
        'Are you sure you want to restore all polling and detection settings to their default values?',
        async () => {
          try {
            await api.resetConfig();
            this.app.showToast('Configuration reset to defaults successfully', 'success');
            await this.load();
          } catch (err) {
            this.app.showToast(`Reset failed: ${err.message}`, 'error');
          }
        }
      );
    });
  }

  async load() {
    try {
      const config = await api.getConfig();

      this.pollingSecondsInput.value = config.polling_seconds ?? 5;
      this.betweenSecondsInput.value = config.between_seconds ?? 30;
      this.recordingClipInput.value = config.recording_clip ?? 30;
      this.sendErrorsSwitch.checked = Boolean(config.send_errors);
      this.autoRenewalSwitch.checked = Boolean(config.auto_renewal);
    } catch (err) {
      console.error('Failed to load general settings:', err);
      this.app.showToast(`Error loading settings: ${err.message}`, 'error');
    }
  }

  async saveSettings() {
    try {
      this.app.showToast('Saving general settings...', 'info');

      const pollingConfig = {
        polling_seconds: parseInt(this.pollingSecondsInput.value, 10),
        between_seconds: parseInt(this.betweenSecondsInput.value, 10),
        recording_clip: parseInt(this.recordingClipInput.value, 10),
        send_errors: this.sendErrorsSwitch.checked,
        auto_renewal: this.autoRenewalSwitch.checked,
      };

      await api.updatePollingConfig(pollingConfig);
      this.app.showToast('General settings saved successfully!', 'success');
    } catch (err) {
      console.error('Failed to save settings:', err);
      this.app.showToast(`Save error: ${err.message}`, 'error');
    }
  }
}
