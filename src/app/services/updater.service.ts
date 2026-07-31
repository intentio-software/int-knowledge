import { Injectable, inject } from '@angular/core';
import { MessageService } from 'primeng/api';

@Injectable({ providedIn: 'root' })
export class UpdaterService {
  private messages = inject(MessageService);

  async checkForUpdates(): Promise<void> {
    if (typeof (window as any).__TAURI_INTERNALS__ === 'undefined') return;
    try {
      const { check } = await import('@tauri-apps/plugin-updater');
      const update = await check();
      if (!update?.available) return;
      this.messages.add({
        severity: 'info',
        summary: `Update available — v${update.version}`,
        detail: 'Click "Update Now" to download and restart.',
        sticky: true,
        data: update,
      });
    } catch (err) {
      console.warn('Update check failed:', err);
    }
  }

  async manualCheck(): Promise<void> {
    if (typeof (window as any).__TAURI_INTERNALS__ === 'undefined') {
      this.messages.add({
        severity: 'info',
        summary: 'Updates unavailable',
        detail: 'Auto-update only works in the desktop app.',
        life: 4000,
      });
      return;
    }
    try {
      const { check } = await import('@tauri-apps/plugin-updater');
      const update = await check();
      if (!update?.available) {
        this.messages.add({
          severity: 'success',
          summary: 'You\'re up to date',
          detail: 'No updates available right now.',
          life: 4000,
        });
        return;
      }
      this.messages.add({
        severity: 'info',
        summary: `Update available — v${update.version}`,
        detail: 'Click "Update Now" to download and restart.',
        sticky: true,
        data: update,
      });
    } catch (err) {
      console.warn('Update check failed:', err);
      const isNetworkError = String(err).toLowerCase().includes('fetch') ||
        String(err).toLowerCase().includes('json');
      this.messages.add({
        severity: 'warn',
        summary: 'Update check failed',
        detail: isNetworkError
          ? 'Could not reach the update server. Try again later.'
          : String(err),
        life: 4000,
      });
    }
  }

  async installUpdate(update: any): Promise<void> {
    try {
      this.messages.clear();
      this.messages.add({
        severity: 'info',
        summary: 'Downloading update…',
        detail: 'The app will restart automatically when ready.',
        sticky: true,
      });

      await update.downloadAndInstall();

      const { relaunch } = await import('@tauri-apps/plugin-process');
      await relaunch();
    } catch (err) {
      console.error('Update failed:', err);
      this.messages.add({
        severity: 'error',
        summary: 'Update failed',
        detail: String(err),
        life: 6000,
      });
    }
  }
}
