import { Module } from '@nestjs/common';
import { ConfigModule } from '@nestjs/config';
import { OrdersModule } from './orders/orders.module';
import { DatabaseModule } from './database/database.module';
import { KafkaModule } from './kafka/kafka.module';
import { AppConfigModule } from './config/config.module';

@Module({
  imports: [
    // Load environment variables
    ConfigModule.forRoot({
      isGlobal: true,
      envFilePath: ['.env.local', '.env'],
    }),

    // Database connection (PostgreSQL via TypeORM)
    DatabaseModule,

    // Kafka producer for event streaming
    KafkaModule,

    // External config service client
    AppConfigModule,

    // Orders domain module
    OrdersModule,
  ],
  controllers: [],
  providers: [],
})
export class AppModule {}
