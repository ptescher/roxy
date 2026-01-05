import {
  Controller,
  Get,
  Post,
  Body,
  Patch,
  Param,
  Delete,
  Query,
  HttpCode,
  HttpStatus,
  Logger,
} from '@nestjs/common';
import { OrdersService, OrderStats, OrdersResponse } from './orders.service';
import { CreateOrderDto } from './dto/create-order.dto';
import { UpdateOrderDto } from './dto/update-order.dto';
import { Order, OrderStatus } from './entities/order.entity';
import { KafkaProducer } from '../kafka/kafka.producer';
import { ConfigServiceClient } from '../config/config.service';
import { DatabaseHealthService } from '../database/database-health.service';

export interface DependencyHealth {
  database: boolean;
  kafka: boolean;
  configService: boolean;
  message: string;
}

@Controller('orders')
export class OrdersController {
  private readonly logger = new Logger(OrdersController.name);

  constructor(
    private readonly ordersService: OrdersService,
    private readonly kafkaProducer: KafkaProducer,
    private readonly configService: ConfigServiceClient,
    private readonly databaseHealth: DatabaseHealthService,
  ) {}

  /**
   * Create a new order
   * POST /api/orders
   */
  @Post()
  @HttpCode(HttpStatus.CREATED)
  async create(@Body() createOrderDto: CreateOrderDto): Promise<Order> {
    this.logger.log(`Creating order for customer: ${createOrderDto.customerId}`);
    return this.ordersService.create(createOrderDto);
  }

  /**
   * Get all orders with optional filtering
   * GET /api/orders
   */
  @Get()
  async findAll(
    @Query('status') status?: OrderStatus,
    @Query('customerId') customerId?: string,
    @Query('limit') limit?: string,
    @Query('offset') offset?: string,
  ): Promise<OrdersResponse> {
    return this.ordersService.findAll({
      status,
      customerId,
      limit: limit ? parseInt(limit, 10) : undefined,
      offset: offset ? parseInt(offset, 10) : undefined,
    });
  }

  /**
   * Get order statistics
   * GET /api/orders/stats/summary
   */
  @Get('stats/summary')
  async getStats(): Promise<OrderStats> {
    return this.ordersService.getStats();
  }

  /**
   * Check health of all dependencies (database, Kafka, config service)
   * GET /api/orders/health/dependencies
   */
  @Get('health/dependencies')
  async checkDependencies(): Promise<DependencyHealth> {
    this.logger.debug('Checking dependency health...');

    const [dbHealthy, kafkaHealthy, configHealthy] = await Promise.all([
      this.databaseHealth.isHealthy(),
      this.kafkaProducer.isHealthy().catch(() => false),
      this.configService.healthCheck().catch(() => false),
    ]);

    const allHealthy = dbHealthy && kafkaHealthy && configHealthy;
    const issues: string[] = [];

    if (!dbHealthy) issues.push('database');
    if (!kafkaHealthy) issues.push('kafka');
    if (!configHealthy) issues.push('config-service');

    const message = allHealthy
      ? 'All dependencies are healthy'
      : `Issues with: ${issues.join(', ')}`;

    return {
      database: dbHealthy,
      kafka: kafkaHealthy,
      configService: configHealthy,
      message,
    };
  }

  /**
   * Get a single order by ID
   * GET /api/orders/:id
   */
  @Get(':id')
  async findOne(@Param('id') id: string): Promise<Order> {
    return this.ordersService.findOne(id);
  }

  /**
   * Update an order
   * PATCH /api/orders/:id
   */
  @Patch(':id')
  async update(
    @Param('id') id: string,
    @Body() updateOrderDto: UpdateOrderDto,
  ): Promise<Order> {
    this.logger.log(`Updating order: ${id}`);
    return this.ordersService.update(id, updateOrderDto);
  }

  /**
   * Confirm an order
   * POST /api/orders/:id/confirm
   */
  @Post(':id/confirm')
  @HttpCode(HttpStatus.OK)
  async confirm(@Param('id') id: string): Promise<Order> {
    this.logger.log(`Confirming order: ${id}`);
    return this.ordersService.confirm(id);
  }

  /**
   * Cancel an order
   * POST /api/orders/:id/cancel
   */
  @Post(':id/cancel')
  @HttpCode(HttpStatus.OK)
  async cancel(
    @Param('id') id: string,
    @Body('reason') reason?: string,
  ): Promise<Order> {
    this.logger.log(`Cancelling order: ${id}`);
    return this.ordersService.cancel(id, reason);
  }

  /**
   * Delete an order (only cancelled orders can be deleted)
   * DELETE /api/orders/:id
   */
  @Delete(':id')
  @HttpCode(HttpStatus.NO_CONTENT)
  async remove(@Param('id') id: string): Promise<void> {
    this.logger.log(`Deleting order: ${id}`);
    return this.ordersService.remove(id);
  }
}
