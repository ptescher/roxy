import { Module } from '@nestjs/common';
import { HttpModule } from '@nestjs/axios';
import { ConfigModule, ConfigService } from '@nestjs/config';
import { ConfigServiceClient } from './config.service';

@Module({
  imports: [
    HttpModule.registerAsync({
      imports: [ConfigModule],
      inject: [ConfigService],
      useFactory: (configService: ConfigService) => ({
        baseURL: configService.get<string>(
          'CONFIG_SERVICE_URL',
          'http://config-service.backend.svc.cluster.local:8080',
        ),
        timeout: 5000,
        headers: {
          'Content-Type': 'application/json',
        },
      }),
    }),
  ],
  providers: [ConfigServiceClient],
  exports: [ConfigServiceClient],
})
export class AppConfigModule {}
