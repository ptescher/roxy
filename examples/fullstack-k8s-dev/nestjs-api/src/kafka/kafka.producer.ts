import {
  Injectable,
  Logger,
  OnModuleInit,
  OnModuleDestroy,
} from "@nestjs/common";
import { ConfigService } from "@nestjs/config";
import { Kafka, Producer, ProducerRecord, RecordMetadata } from "kafkajs";
import { SocksClient } from "socks";
import * as net from "net";
import * as tls from "tls";

/**
 * Kafka producer service for publishing events
 *
 * Supports SOCKS5 proxy for connecting to Kubernetes services via Roxy.
 *
 * To use with Roxy's SOCKS5 proxy:
 *   SOCKS5_PROXY=socks5://127.0.0.1:1080 npm run start:dev
 *
 * This allows connecting to K8s service DNS names like:
 *   kafka.backend.svc.cluster.local:9092
 *
 * Roxy will forward the connection to the actual Kubernetes service.
 */
@Injectable()
export class KafkaProducer implements OnModuleInit, OnModuleDestroy {
  private readonly logger = new Logger(KafkaProducer.name);
  private kafka: Kafka;
  private producer: Producer;
  private isConnected = false;

  constructor(private readonly configService: ConfigService) {
    const brokers = this.configService
      .get<string>("KAFKA_BROKERS", "kafka.messaging.svc.cluster.local:9092")
      .split(",");

    const socksProxyUrl = this.configService.get<string>("SOCKS5_PROXY", "");
    const socksProxy = socksProxyUrl
      ? this.parseSocksProxy(socksProxyUrl)
      : null;

    // Configure Kafka client
    const kafkaConfig: ConstructorParameters<typeof Kafka>[0] = {
      clientId: this.configService.get<string>(
        "KAFKA_CLIENT_ID",
        "nestjs-orders-api",
      ),
      brokers,
      connectionTimeout: 10000,
      retry: {
        initialRetryTime: 1000,
        retries: 5,
      },
    };

    // If SOCKS proxy is configured, add custom socket factory
    if (socksProxy) {
      this.logger.log(
        `Using SOCKS5 proxy at ${socksProxy.host}:${socksProxy.port} for Kafka connections`,
      );
      this.logger.log(
        `Kafka brokers: ${brokers.join(", ")} (will be resolved by Roxy)`,
      );

      kafkaConfig.socketFactory = ({ host, port, ssl, onConnect }) => {
        const useSsl = ssl !== undefined && ssl !== null;
        const socket = this.createSocksSocket(
          socksProxy,
          host,
          port,
          useSsl,
          ssl,
        );

        socket
          .then((sock) => {
            sock.on("connect", () => {
              onConnect();
            });
          })
          .catch((err) => {
            this.logger.error(
              `SOCKS5 connection to ${host}:${port} failed: ${err.message}`,
            );
          });

        // Return a placeholder socket that will be replaced
        // KafkaJS expects a socket-like object immediately
        return this.createProxiedSocket(socket);
      };
    } else {
      this.logger.log(`Kafka brokers: ${brokers.join(", ")}`);

      if (brokers.some((b) => b.includes(".svc.cluster.local"))) {
        this.logger.warn(
          "Kafka broker is a Kubernetes DNS name but no SOCKS5_PROXY is configured",
        );
        this.logger.log(
          "Tip: Set SOCKS5_PROXY=socks5://127.0.0.1:1080 to route through Roxy",
        );
      }
    }

    this.kafka = new Kafka(kafkaConfig);

    this.producer = this.kafka.producer({
      allowAutoTopicCreation: true,
      transactionTimeout: 30000,
    });
  }

  /**
   * Parse a SOCKS5 proxy URL into host and port
   */
  private parseSocksProxy(
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
   * Create a socket connection through SOCKS5 proxy
   */
  private async createSocksSocket(
    proxy: { host: string; port: number },
    targetHost: string,
    targetPort: number,
    useSsl: boolean,
    sslOptions?: tls.ConnectionOptions,
  ): Promise<net.Socket> {
    const { socket } = await SocksClient.createConnection({
      proxy: {
        host: proxy.host,
        port: proxy.port,
        type: 5, // SOCKS5
      },
      command: "connect",
      destination: {
        host: targetHost,
        port: targetPort,
      },
    });

    if (useSsl) {
      // Upgrade to TLS if needed
      return tls.connect({
        ...sslOptions,
        socket,
        servername: targetHost,
      });
    }

    return socket;
  }

  /**
   * Create a proxied socket that wraps an async socket creation
   */
  private createProxiedSocket(socketPromise: Promise<net.Socket>): net.Socket {
    // Create a pass-through socket
    const passthrough = new net.Socket();

    socketPromise
      .then((actualSocket) => {
        // Pipe data between passthrough and actual socket
        actualSocket.pipe(passthrough);
        passthrough.pipe(actualSocket);

        // Forward events
        actualSocket.on("error", (err) => passthrough.emit("error", err));
        actualSocket.on("close", () => passthrough.destroy());
        passthrough.on("close", () => actualSocket.destroy());

        // Emit connect event
        passthrough.emit("connect");
      })
      .catch((err) => {
        passthrough.emit("error", err);
        passthrough.destroy();
      });

    return passthrough;
  }

  async onModuleInit() {
    try {
      await this.connect();
    } catch (error) {
      this.logger.warn(
        `Failed to connect to Kafka on startup: ${error.message}. Will retry on first publish.`,
      );
    }
  }

  async onModuleDestroy() {
    await this.disconnect();
  }

  /**
   * Connect to Kafka brokers
   */
  async connect(): Promise<void> {
    if (this.isConnected) {
      return;
    }

    this.logger.debug("Connecting to Kafka...");
    try {
      await this.producer.connect();
      this.isConnected = true;
      this.logger.log("Connected to Kafka");
    } catch (error) {
      this.logger.error(`Failed to connect to Kafka: ${error.message}`);
      throw error;
    }
  }

  /**
   * Disconnect from Kafka
   */
  async disconnect(): Promise<void> {
    if (!this.isConnected) {
      return;
    }

    this.logger.debug("Disconnecting from Kafka...");
    try {
      await this.producer.disconnect();
      this.isConnected = false;
      this.logger.log("Disconnected from Kafka");
    } catch (error) {
      this.logger.error(`Error disconnecting from Kafka: ${error.message}`);
    }
  }

  /**
   * Publish a message to a Kafka topic
   */
  async publish<T = any>(
    topic: string,
    message: T,
    key?: string,
  ): Promise<RecordMetadata[]> {
    // Ensure we're connected
    if (!this.isConnected) {
      await this.connect();
    }

    const record: ProducerRecord = {
      topic,
      messages: [
        {
          key: key || undefined,
          value: JSON.stringify(message),
          timestamp: Date.now().toString(),
        },
      ],
    };

    this.logger.debug(
      `Publishing to topic '${topic}': ${JSON.stringify(message)}`,
    );

    try {
      const result = await this.producer.send(record);
      this.logger.debug(`Published to '${topic}' successfully`);
      return result;
    } catch (error) {
      this.logger.error(`Failed to publish to '${topic}': ${error.message}`);
      throw error;
    }
  }

  /**
   * Publish an order event
   */
  async publishOrderEvent(
    eventType: "created" | "confirmed" | "cancelled" | "shipped" | "delivered",
    orderId: string,
    data: any,
  ): Promise<void> {
    const topic = this.configService.get<string>(
      "KAFKA_ORDERS_TOPIC",
      "orders",
    );

    const event = {
      eventType,
      orderId,
      timestamp: new Date().toISOString(),
      data,
    };

    await this.publish(topic, event, orderId);
  }

  /**
   * Check if Kafka connection is healthy
   */
  async isHealthy(): Promise<boolean> {
    if (!this.isConnected) {
      try {
        await this.connect();
      } catch {
        return false;
      }
    }
    return this.isConnected;
  }

  /**
   * Get connection status
   */
  getStatus(): { connected: boolean; brokers: string[] } {
    const brokers = this.configService
      .get<string>("KAFKA_BROKERS", "kafka.backend.svc.cluster.local:9092")
      .split(",");

    return {
      connected: this.isConnected,
      brokers,
    };
  }
}
