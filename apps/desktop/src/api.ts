import { invoke } from "@tauri-apps/api/core";
import type { DeviceInfo } from "./types";

// Pembungkus tipis invoke agar komponen tidak mengimpor @tauri-apps/api
// di banyak tempat.

export function deviceInfo(): Promise<DeviceInfo> {
  return invoke<DeviceInfo>("device_info");
}

export function hostStart(serverUrl: string): Promise<void> {
  return invoke("host_start", { serverUrl });
}

export function hostRespond(
  sessionId: string,
  approved: boolean,
  permissions: string[],
): Promise<void> {
  return invoke("host_respond", { sessionId, approved, permissions });
}

export function hostEnd(reason?: string): Promise<void> {
  return invoke("host_end", { reason: reason ?? null });
}

export function controllerConnect(
  serverUrl: string,
  targetDeviceId: string,
  mode: "attended" | "unattended",
  permissions: string[],
): Promise<void> {
  return invoke("controller_connect", {
    serverUrl,
    targetDeviceId,
    mode,
    permissions,
  });
}

export function controllerMouse(
  nx: number,
  ny: number,
  action: "press" | "release" | "move" | "scroll",
  button: "left" | "right" | "middle",
  scrollDx: number,
  scrollDy: number,
): Promise<void> {
  return invoke("controller_mouse", {
    nx,
    ny,
    action,
    button,
    scrollDx,
    scrollDy,
  });
}

export function controllerKeyboard(
  action: "press" | "release",
  key: string,
  modifiers: string[],
): Promise<void> {
  return invoke("controller_keyboard", { action, key, modifiers });
}

export function controllerEnd(reason?: string): Promise<void> {
  return invoke("controller_end", { reason: reason ?? null });
}
