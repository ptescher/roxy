declare module 'global-agent' {
  export function bootstrap(): void;
  export function createGlobalProxyAgent(options?: {
    environmentVariableNamespace?: string;
    forceGlobalAgent?: boolean;
    socketConnectionTimeout?: number;
  }): void;
}
