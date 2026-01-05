import { bootstrap as bootstrapGlobalAgent } from 'global-agent';
import { NestFactory } from '@nestjs/core';
import { ConfigService } from '@nestjs/config';
import { Logger, ValidationPipe } from '@nestjs/common';
import { AppModule } from './app.module';

// Enable global-agent for HTTP_PROXY support
if (process.env.HTTP_PROXY || process.env.HTTPS_PROXY) {
  bootstrapGlobalAgent();
  console.log('🔌 Proxy enabled via global-agent');
  console.log(`   HTTP_PROXY=${process.env.HTTP_PROXY || 'not set'}`);
  console.log(`   HTTPS_PROXY=${process.env.HTTPS_PROXY || 'not set'}`);
  console.log(`   NO_PROXY=${process.env.NO_PROXY || 'not set'}`);
}

async function bootstrap() {
  const logger = new Logger('Bootstrap');

  const app = await NestFactory.create(AppModule, {
    logger: ['error', 'warn', 'log', 'debug', 'verbose'],
  });

  const configService = app.get(ConfigService);
  const port = configService.get<number>('PORT', 3000);

  // Enable CORS for React Native app
  app.enableCors({
    origin: true,
    methods: ['GET', 'POST', 'PUT', 'DELETE', 'PATCH', 'OPTIONS'],
    allowedHeaders: ['Content-Type', 'Authorization', 'X-Request-ID'],
    credentials: true,
  });

  // Validation
  app.useGlobalPipes(
    new ValidationPipe({
      whitelist: true,
      transform: true,
      forbidNonWhitelisted: true,
    }),
  );

  // Global API prefix
  app.setGlobalPrefix('api');

  await app.listen(port);

  logger.log(`🚀 Application is running on: http://localhost:${port}/api`);
  logger.log('');
  logger.log('📡 Kubernetes service endpoints:');
  logger.log(
    `   • PostgreSQL: ${configService.get('DATABASE_HOST', 'localhost')}:${configService.get('DATABASE_PORT', 5432)}`,
  );
  logger.log(`   • Kafka: ${configService.get('KAFKA_BROKERS', 'localhost:9092')}`);
  logger.log(
    `   • Config Service: ${configService.get('CONFIG_SERVICE_URL', 'http://localhost:8080')}`,
  );
  logger.log('');

  if (process.env.HTTP_PROXY || process.env.HTTPS_PROXY) {
    logger.log('🔌 Proxy mode: Outbound requests routed through Roxy');
    logger.log('   Roxy will auto-forward *.svc.cluster.local to K8s');
  } else {
    logger.log('💡 To enable K8s auto-forwarding, set HTTP_PROXY=http://127.0.0.1:8080');
  }
}

bootstrap();
