import { Module } from '@nestjs/common';
import { ConfigModule } from '@nestjs/config';
import { KafkaProducer } from './kafka.producer';

@Module({
  imports: [ConfigModule],
  providers: [KafkaProducer],
  exports: [KafkaProducer],
})
export class KafkaModule {}
