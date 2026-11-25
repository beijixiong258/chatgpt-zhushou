type Reply = { sequence: number; value?: unknown; error?: string };

interface DesktopPort {
  postMessage(message: object): void;
  addEventListener(type: "message", listener: (event: MessageEvent<Reply>) => void): void;
}

declare global {
  interface Window {
    chrome?: { webview?: DesktopPort };
  }
}

const port = window.chrome?.webview;
let sequence = 0;
const pending = new Map<number, {
  resolve(value: unknown): void;
  reject(reason: Error): void;
}>();

port?.addEventListener("message", ({ data }) => {
  const request = pending.get(data.sequence);
  if (!request) return;
  pending.delete(data.sequence);
  if (data.error !== undefined) request.reject(new Error(data.error));
  else request.resolve(data.value);
});

export function invoke<T>(method: string, fields: Record<string, unknown> = {}): Promise<T> {
  return new Promise((resolve, reject) => {
    if (!port) throw new Error("请在桌面程序中打开会话编辑器");
    const id = ++sequence;
    pending.set(id, { resolve: (value) => resolve(value as T), reject });
    try {
      port.postMessage({ sequence: id, method, ...fields });
    } catch (error) {
      pending.delete(id);
      reject(error);
    }
  });
}

export function setWindowState(dirty: boolean, busy: boolean) {
  port?.postMessage({ windowState: { dirty, busy } });
}
