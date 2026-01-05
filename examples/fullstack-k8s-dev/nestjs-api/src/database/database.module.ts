import { Module, Logger } from "@nestjs/common";
import { TypeOrmModule } from "@nestjs/typeorm";
import { ConfigModule, ConfigService } from "@nestjs/config";
import { DatabaseHealthService } from "./database-health.service";
import { SocksClient } from "socks";
import * as net from "net";
import { Duplex } from "stream";

/**
 * Database module for PostgreSQL connection via TypeORM
 *
 * Supports SOCKS5 proxy for connecting to Kubernetes services via Roxy.
 *
 * To use with Roxy's SOCKS5 proxy:
 *   SOCKS5_PROXY=socks5://127.0.0.1:1080 npm run start:dev
 *
 * This allows connecting to K8s service DNS names like:
 *   postgres.database.svc.cluster.local
 *
 * Roxy will forward the connection to the actual Kubernetes service.
 */

/**
 * Parse a SOCKS5 proxy URL into host and port
 */
function parseSocksProxy(
  proxyUrl: string,
): { host: string; port: number } | null {
  try {
    // Handle socks5://host:port format
    const url = new URL(proxyUrl);
    if (url.protocol !== "socks5:" && url.protocol !== "socks:") {
      return null;
    }
    return {
      host: url.hostname || "127.0.0.1",
      port: parseInt(url.port, 10) || 1080,
    };
  } catch {
    // Handle host:port format without protocol
    const parts = proxyUrl.split(":");
    if (parts.length === 2) {
      return {
        host: parts[0] || "127.0.0.1",
        port: parseInt(parts[1], 10) || 1080,
      };
    }
    return null;
  }
}

/**
 * A socket wrapper that connects through SOCKS5 proxy.
 * This class mimics net.Socket interface but routes through SOCKS.
 */
class SocksSocket extends Duplex {
  private socket: net.Socket | null = null;
  private proxy: { host: string; port: number };
  private logger: Logger;
  private connected = false;
  private pendingData: Buffer[] = [];

  constructor(proxy: { host: string; port: number }) {
    super();
    this.proxy = proxy;
    this.logger = new Logger("SocksSocket");
  }

  // pg calls setNoDelay before connect - we need to handle this
  setNoDelay(noDelay?: boolean): this {
    if (this.socket) {
      this.socket.setNoDelay(noDelay);
    }
    return this;
  }

  setKeepAlive(enable?: boolean, initialDelay?: number): this {
    if (this.socket) {
      this.socket.setKeepAlive(enable, initialDelay);
    }
    return this;
  }

  // pg calls connect(port, host) to establish the connection
  connect(port: number, host: string): this;
  connect(options: net.SocketConnectOpts): this;
  connect(portOrOptions: number | net.SocketConnectOpts, host?: string): this {
    let targetPort: number;
    let targetHost: string;

    if (typeof portOrOptions === "number") {
      targetPort = portOrOptions;
      targetHost = host || "localhost";
    } else {
      targetPort = (portOrOptions as net.TcpSocketConnectOpts).port || 5432;
      targetHost =
        (portOrOptions as net.TcpSocketConnectOpts).host || "localhost";
    }

    this.logger.debug(
      `Connecting to ${targetHost}:${targetPort} via SOCKS5 proxy ${this.proxy.host}:${this.proxy.port}`,
    );

    // Connect through SOCKS proxy
    SocksClient.createConnection({
      proxy: {
        host: this.proxy.host,
        port: this.proxy.port,
        type: 5, // SOCKS5
      },
      command: "connect",
      destination: {
        host: targetHost,
        port: targetPort,
      },
      timeout: 30000,
    })
      .then(({ socket }) => {
        this.socket = socket;
        this.connected = true;

        // Set socket options
        this.socket.setNoDelay(true);

        // Forward data from the real socket to this Duplex stream
        this.socket.on("data", (data) => {
          if (!this.push(data)) {
            this.socket?.pause();
          }
        });

        this.socket.on("end", () => {
          this.push(null);
        });

        this.socket.on("error", (err) => {
          this.logger.error(`Socket error: ${err.message}`);
          this.emit("error", err);
        });

        this.socket.on("close", (hadError) => {
          this.emit("close", hadError);
        });

        // Flush any pending writes
        for (const data of this.pendingData) {
          this.socket.write(data);
        }
        this.pendingData = [];

        // Emit connect event
        this.logger.debug(
          `Connected to ${targetHost}:${targetPort} via SOCKS5`,
        );
        this.emit("connect");
      })
      .catch((err) => {
        this.logger.error(`SOCKS5 connection failed: ${err.message}`);
        this.emit("error", err);
      });

    return this;
  }

  _read(size: number): void {
    // Resume the underlying socket if paused
    if (this.socket) {
      this.socket.resume();
    }
  }

  _write(
    chunk: Buffer,
    encoding: BufferEncoding,
    callback: (error?: Error | null) => void,
  ): void {
    if (this.socket && this.connected) {
      this.socket.write(chunk, encoding, callback);
    } else {
      // Queue data until connected
      this.pendingData.push(chunk);
      callback();
    }
  }

  _destroy(
    error: Error | null,
    callback: (error?: Error | null) => void,
  ): void {
    if (this.socket) {
      this.socket.destroy(error || undefined);
    }
    callback(error);
  }

  // Additional methods that pg might call
  end(callback?: () => void): this {
    if (this.socket) {
      this.socket.end(callback);
    } else if (callback) {
      callback();
    }
    return this;
  }

  destroy(error?: Error): this {
    if (this.socket) {
      this.socket.destroy(error);
    }
    return this;
  }

  get readyState(): string {
    if (!this.socket) return "opening";
    return this.connected ? "open" : "opening";
  }

  // ref/unref methods required by pg library to control event loop
  ref(): this {
    if (this.socket) {
      this.socket.ref();
    }
    return this;
  }

  unref(): this {
    if (this.socket) {
      this.socket.unref();
    }
    return this;
  }
}

/**
 * Create a stream factory for pg that routes through SOCKS5 proxy
 */
function createSocksStreamFactory(proxy: { host: string; port: number }) {
  return function streamFactory(): SocksSocket {
    return new SocksSocket(proxy);
  };
}

@Module({
  imports: [
    TypeOrmModule.forRootAsync({
      imports: [ConfigModule],
      inject: [ConfigService],
      useFactory: (configService: ConfigService) => {
        const logger = new Logger("DatabaseModule");
        const host = configService.get<string>(
          "DATABASE_HOST",
          "postgres.database.svc.cluster.local",
        );
        const port = configService.get<number>("DATABASE_PORT", 5432);
        const socksProxyUrl = configService.get<string>("SOCKS5_PROXY", "");

        // Parse SOCKS proxy configuration
        const socksProxy = socksProxyUrl
          ? parseSocksProxy(socksProxyUrl)
          : null;

        if (socksProxy) {
          logger.log(
            `Using SOCKS5 proxy at ${socksProxy.host}:${socksProxy.port} for database connections`,
          );
          logger.log(
            `Database target: ${host}:${port} (will be resolved by Roxy)`,
          );
        } else if (host.includes(".svc.cluster.local")) {
          logger.warn(
            "Database host is a Kubernetes DNS name but no SOCKS5_PROXY is configured",
          );
          logger.log(
            "Tip: Set SOCKS5_PROXY=socks5://127.0.0.1:1080 to route through Roxy",
          );
        }

        const baseConfig = {
          type: "postgres" as const,
          host,
          port,
          username: configService.get<string>("DATABASE_USER", "app"),
          password: configService.get<string>("DATABASE_PASSWORD", "password"),
          database: configService.get<string>("DATABASE_NAME", "orders"),
          entities: [__dirname + "/../**/*.entity{.ts,.js}"],
          synchronize: configService.get<boolean>("DATABASE_SYNC", true),
          logging: configService.get<boolean>("DATABASE_LOGGING", false),
          retryAttempts: 5,
          retryDelay: 3000,
          autoLoadEntities: true,
        };

        // If SOCKS proxy is configured, add custom stream factory
        if (socksProxy) {
          return {
            ...baseConfig,
            extra: {
              // Custom stream factory that routes through SOCKS5 proxy
              stream: createSocksStreamFactory(socksProxy),
            },
          };
        }

        return baseConfig;
      },
    }),
  ],
  providers: [DatabaseHealthService],
  exports: [DatabaseHealthService],
})
export class DatabaseModule {}
