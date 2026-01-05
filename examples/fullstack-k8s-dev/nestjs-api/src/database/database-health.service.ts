import { Injectable, Logger } from '@nestjs/common';
import { DataSource } from 'typeorm';

@Injectable()
export class DatabaseHealthService {
  private readonly logger = new Logger(DatabaseHealthService.name);

  constructor(private readonly dataSource: DataSource) {}

  /**
   * Check if the database connection is healthy
   */
  async isHealthy(): Promise<boolean> {
    try {
      // Try to execute a simple query
      await this.dataSource.query('SELECT 1');
      return true;
    } catch (error) {
      this.logger.error(`Database health check failed: ${error.message}`);
      return false;
    }
  }

  /**
   * Get database connection info
   */
  getConnectionInfo(): {
    host: string;
    port: number;
    database: string;
    isConnected: boolean;
  } {
    const options = this.dataSource.options as any;
    return {
      host: options.host || 'unknown',
      port: options.port || 5432,
      database: options.database || 'unknown',
      isConnected: this.dataSource.isInitialized,
    };
  }

  /**
   * Execute a raw query (for debugging)
   */
  async rawQuery<T = any>(query: string, parameters?: any[]): Promise<T> {
    this.logger.debug(`Executing raw query: ${query}`);
    return this.dataSource.query(query, parameters);
  }
}
