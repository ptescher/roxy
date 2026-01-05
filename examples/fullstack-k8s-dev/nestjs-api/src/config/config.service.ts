import { Injectable, Logger } from "@nestjs/common";
import { HttpService } from "@nestjs/axios";
import { ConfigService } from "@nestjs/config";
import { firstValueFrom, timeout, catchError } from "rxjs";
import { AxiosError, AxiosResponse } from "axios";

/**
 * Feature flags returned by the config service
 */
export interface FeatureFlags {
  newCheckoutEnabled: boolean;
  inventoryCheckEnabled: boolean;
  notificationsEnabled: boolean;
  analyticsEnabled: boolean;
}

/**
 * Application configuration from the config service
 */
export interface AppConfig {
  orderLimits: {
    maxItemsPerOrder: number;
    maxOrderValue: number;
    minOrderValue: number;
  };
  taxation: {
    defaultTaxRate: number;
    taxExemptThreshold: number;
  };
  shipping: {
    freeShippingThreshold: number;
    defaultShippingCost: number;
  };
}

/**
 * Client for communicating with the config-service in Kubernetes
 *
 * The config-service runs at config-service.backend.svc.cluster.local:8080
 * Roxy automatically handles port forwarding to the K8s cluster!
 *
 * This means you can use the K8s DNS name directly in your code,
 * and Roxy will transparently route traffic through a port-forward.
 */
@Injectable()
export class ConfigServiceClient {
  private readonly logger = new Logger(ConfigServiceClient.name);
  private readonly serviceUrl: string;

  // Cache for feature flags (to reduce calls to config service)
  private featureFlagsCache: FeatureFlags | null = null;
  private featureFlagsCacheExpiry: number = 0;
  private readonly cacheTtlMs = 60000; // 1 minute cache

  constructor(
    private readonly httpService: HttpService,
    private readonly configService: ConfigService,
  ) {
    this.serviceUrl = this.configService.get<string>(
      "CONFIG_SERVICE_URL",
      "http://config-service.backend.svc.cluster.local:8080",
    );

    this.logger.log(`Config Service URL: ${this.serviceUrl}`);
    this.logger.log(
      "Note: Roxy will auto-forward this K8s address to the cluster",
    );
  }

  /**
   * Fetch feature flags from the config service
   *
   * Uses caching to reduce load on the config service.
   * Cache TTL is 1 minute by default.
   */
  async getFeatureFlags(): Promise<FeatureFlags> {
    // Check cache first
    if (this.featureFlagsCache && Date.now() < this.featureFlagsCacheExpiry) {
      this.logger.debug("Returning cached feature flags");
      return this.featureFlagsCache;
    }

    this.logger.debug("Fetching feature flags from config-service");

    try {
      const response: AxiosResponse<FeatureFlags> = await firstValueFrom(
        this.httpService.get<FeatureFlags>("/api/v1/features").pipe(
          timeout(5000),
          catchError((error: AxiosError) => {
            this.logger.error(
              `Failed to fetch feature flags: ${error.message}`,
              error.stack,
            );
            throw error;
          }),
        ),
      );

      // Update cache
      this.featureFlagsCache = response.data;
      this.featureFlagsCacheExpiry = Date.now() + this.cacheTtlMs;

      this.logger.debug(
        `Feature flags fetched: ${JSON.stringify(response.data)}`,
      );
      return response.data;
    } catch (error) {
      // Return default feature flags if service is unavailable
      this.logger.warn(
        "Config service unavailable, using default feature flags",
      );
      return this.getDefaultFeatureFlags();
    }
  }

  /**
   * Get application configuration from the config service
   */
  async getAppConfig(): Promise<AppConfig> {
    this.logger.debug("Fetching app config from config-service");

    try {
      const response: AxiosResponse<AppConfig> = await firstValueFrom(
        this.httpService.get<AppConfig>("/api/v1/config/app").pipe(
          timeout(5000),
          catchError((error: AxiosError) => {
            this.logger.error(
              `Failed to fetch app config: ${error.message}`,
              error.stack,
            );
            throw error;
          }),
        ),
      );

      this.logger.debug("App config fetched successfully");
      return response.data;
    } catch (error) {
      this.logger.warn("Config service unavailable, using default app config");
      return this.getDefaultAppConfig();
    }
  }

  /**
   * Get a specific configuration value by key
   */
  async getConfigValue<T>(key: string, defaultValue: T): Promise<T> {
    this.logger.debug(`Fetching config value: ${key}`);

    try {
      const response: AxiosResponse<{ value: T }> = await firstValueFrom(
        this.httpService.get<{ value: T }>(`/api/v1/config/${key}`).pipe(
          timeout(5000),
          catchError((error: AxiosError) => {
            this.logger.error(
              `Failed to fetch config value '${key}': ${error.message}`,
            );
            throw error;
          }),
        ),
      );

      return response.data.value;
    } catch (error) {
      this.logger.warn(`Using default value for config key "${key}"`);
      return defaultValue;
    }
  }

  /**
   * Health check for the config service
   *
   * This is used to verify connectivity to the K8s service.
   * Roxy handles the port forwarding automatically.
   */
  async healthCheck(): Promise<boolean> {
    this.logger.debug("Checking config-service health");

    try {
      const response: AxiosResponse = await firstValueFrom(
        this.httpService.get("/health").pipe(
          timeout(3000),
          catchError((error: AxiosError) => {
            this.logger.error(
              `Config service health check failed: ${error.message}`,
            );
            throw error;
          }),
        ),
      );

      const isHealthy = response.status === 200;
      this.logger.debug(`Config service health: ${isHealthy ? "OK" : "FAIL"}`);
      return isHealthy;
    } catch (error) {
      return false;
    }
  }

  /**
   * Invalidate the feature flags cache
   *
   * Call this when you know feature flags have changed
   * (e.g., after receiving a webhook notification)
   */
  invalidateCache(): void {
    this.logger.debug("Invalidating feature flags cache");
    this.featureFlagsCache = null;
    this.featureFlagsCacheExpiry = 0;
  }

  /**
   * Get service info (for debugging)
   */
  getServiceInfo(): { url: string; cacheEnabled: boolean; cacheTtlMs: number } {
    return {
      url: this.serviceUrl,
      cacheEnabled: true,
      cacheTtlMs: this.cacheTtlMs,
    };
  }

  // Private helper methods

  private getDefaultFeatureFlags(): FeatureFlags {
    return {
      newCheckoutEnabled: false,
      inventoryCheckEnabled: true,
      notificationsEnabled: true,
      analyticsEnabled: false,
    };
  }

  private getDefaultAppConfig(): AppConfig {
    return {
      orderLimits: {
        maxItemsPerOrder: 100,
        maxOrderValue: 10000,
        minOrderValue: 1,
      },
      taxation: {
        defaultTaxRate: 0.08,
        taxExemptThreshold: 0,
      },
      shipping: {
        freeShippingThreshold: 50,
        defaultShippingCost: 5.99,
      },
    };
  }
}
