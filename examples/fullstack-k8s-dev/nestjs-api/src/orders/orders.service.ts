import {
  Injectable,
  Logger,
  NotFoundException,
  BadRequestException,
} from "@nestjs/common";
import { InjectRepository } from "@nestjs/typeorm";
import { Repository } from "typeorm";
import { Order, OrderStatus } from "./entities/order.entity";
import { OrderItem } from "./entities/order-item.entity";
import { CreateOrderDto } from "./dto/create-order.dto";
import { UpdateOrderDto } from "./dto/update-order.dto";
import { KafkaProducer } from "../kafka/kafka.producer";
import { ConfigServiceClient } from "../config/config.service";

export interface OrderStats {
  totalOrders: number;
  pendingOrders: number;
  completedOrders: number;
  totalRevenue: number;
}

export interface OrdersResponse {
  orders: Order[];
  total: number;
}

@Injectable()
export class OrdersService {
  private readonly logger = new Logger(OrdersService.name);

  constructor(
    @InjectRepository(Order)
    private readonly orderRepository: Repository<Order>,
    @InjectRepository(OrderItem)
    private readonly orderItemRepository: Repository<OrderItem>,
    private readonly kafkaProducer: KafkaProducer,
    private readonly configService: ConfigServiceClient,
  ) {}

  /**
   * Create a new order
   */
  async create(createOrderDto: CreateOrderDto): Promise<Order> {
    this.logger.log(
      `Creating order for customer: ${createOrderDto.customerId}`,
    );

    // Get app config for tax rate
    const appConfig = await this.configService.getAppConfig();
    const taxRate = appConfig.taxation.defaultTaxRate;

    // Create order entity
    const order = this.orderRepository.create({
      customerId: createOrderDto.customerId,
      customerEmail: createOrderDto.customerEmail,
      status: "pending",
      currency: createOrderDto.currency || "USD",
      shippingAddress: createOrderDto.shippingAddress || null,
      billingAddress: createOrderDto.billingAddress || null,
      notes: createOrderDto.notes || null,
      items: [],
    });

    // Create order items
    order.items = createOrderDto.items.map((itemDto) => {
      const item = new OrderItem();
      item.productId = itemDto.productId;
      item.productName = itemDto.productName;
      item.productSku = itemDto.productSku;
      item.quantity = itemDto.quantity;
      item.unitPrice = itemDto.unitPrice;
      item.discount = itemDto.discount || 0;
      return item;
    });

    // Calculate totals
    order.calculateTotals(taxRate);

    // Validate order limits
    if (order.items.length > appConfig.orderLimits.maxItemsPerOrder) {
      throw new BadRequestException(
        `Order exceeds maximum items limit of ${appConfig.orderLimits.maxItemsPerOrder}`,
      );
    }

    if (order.total > appConfig.orderLimits.maxOrderValue) {
      throw new BadRequestException(
        `Order total exceeds maximum value of ${appConfig.orderLimits.maxOrderValue}`,
      );
    }

    if (order.total < appConfig.orderLimits.minOrderValue) {
      throw new BadRequestException(
        `Order total is below minimum value of ${appConfig.orderLimits.minOrderValue}`,
      );
    }

    // Save order
    const savedOrder = await this.orderRepository.save(order);
    this.logger.log(`Order created: ${savedOrder.id}`);

    // Publish event to Kafka
    try {
      await this.kafkaProducer.publishOrderEvent("created", savedOrder.id, {
        customerId: savedOrder.customerId,
        total: savedOrder.total,
        itemCount: savedOrder.items.length,
      });
    } catch (error) {
      this.logger.warn(
        `Failed to publish order created event: ${error.message}`,
      );
      // Don't fail the order creation if Kafka is unavailable
    }

    return savedOrder;
  }

  /**
   * Find all orders with optional filtering
   */
  async findAll(params?: {
    status?: OrderStatus;
    customerId?: string;
    limit?: number;
    offset?: number;
  }): Promise<OrdersResponse> {
    const { status, customerId, limit = 20, offset = 0 } = params || {};

    const queryBuilder = this.orderRepository
      .createQueryBuilder("order")
      .leftJoinAndSelect("order.items", "items")
      .orderBy("order.createdAt", "DESC")
      .skip(offset)
      .take(limit);

    if (status) {
      queryBuilder.andWhere("order.status = :status", { status });
    }

    if (customerId) {
      queryBuilder.andWhere("order.customerId = :customerId", { customerId });
    }

    const [orders, total] = await queryBuilder.getManyAndCount();

    return { orders, total };
  }

  /**
   * Find a single order by ID
   */
  async findOne(id: string): Promise<Order> {
    const order = await this.orderRepository.findOne({
      where: { id },
      relations: ["items"],
    });

    if (!order) {
      throw new NotFoundException(`Order with ID ${id} not found`);
    }

    return order;
  }

  /**
   * Update an order
   */
  async update(id: string, updateOrderDto: UpdateOrderDto): Promise<Order> {
    const order = await this.findOne(id);

    // Update basic fields
    if (updateOrderDto.notes !== undefined) {
      order.notes = updateOrderDto.notes;
    }
    if (updateOrderDto.shippingAddress !== undefined) {
      order.shippingAddress = updateOrderDto.shippingAddress;
    }
    if (updateOrderDto.billingAddress !== undefined) {
      order.billingAddress = updateOrderDto.billingAddress;
    }

    return this.orderRepository.save(order);
  }

  /**
   * Confirm an order
   */
  async confirm(id: string): Promise<Order> {
    const order = await this.findOne(id);

    if (order.status !== "pending") {
      throw new BadRequestException(
        `Cannot confirm order in status '${order.status}'. Order must be pending.`,
      );
    }

    order.status = "confirmed";
    const savedOrder = await this.orderRepository.save(order);

    // Publish event
    try {
      await this.kafkaProducer.publishOrderEvent("confirmed", savedOrder.id, {
        customerId: savedOrder.customerId,
        total: savedOrder.total,
      });
    } catch (error) {
      this.logger.warn(
        `Failed to publish order confirmed event: ${error.message}`,
      );
    }

    this.logger.log(`Order confirmed: ${savedOrder.id}`);
    return savedOrder;
  }

  /**
   * Cancel an order
   */
  async cancel(id: string, reason?: string): Promise<Order> {
    const order = await this.findOne(id);

    if (["shipped", "delivered", "cancelled"].includes(order.status)) {
      throw new BadRequestException(
        `Cannot cancel order in status '${order.status}'.`,
      );
    }

    order.status = "cancelled";
    if (reason) {
      order.notes = order.notes
        ? `${order.notes}\n\nCancellation reason: ${reason}`
        : `Cancellation reason: ${reason}`;
    }

    const savedOrder = await this.orderRepository.save(order);

    // Publish event
    try {
      await this.kafkaProducer.publishOrderEvent("cancelled", savedOrder.id, {
        customerId: savedOrder.customerId,
        reason,
      });
    } catch (error) {
      this.logger.warn(
        `Failed to publish order cancelled event: ${error.message}`,
      );
    }

    this.logger.log(`Order cancelled: ${savedOrder.id}`);
    return savedOrder;
  }

  /**
   * Delete an order
   */
  async remove(id: string): Promise<void> {
    const order = await this.findOne(id);

    if (order.status !== "cancelled") {
      throw new BadRequestException("Only cancelled orders can be deleted");
    }

    await this.orderRepository.remove(order);
    this.logger.log(`Order deleted: ${id}`);
  }

  /**
   * Get order statistics
   */
  async getStats(): Promise<OrderStats> {
    const totalOrders = await this.orderRepository.count();

    const pendingOrders = await this.orderRepository.count({
      where: { status: "pending" },
    });

    const completedOrders = await this.orderRepository.count({
      where: { status: "delivered" },
    });

    const revenueResult = await this.orderRepository
      .createQueryBuilder("order")
      .select("SUM(order.total)", "totalRevenue")
      .where("order.status IN (:...statuses)", {
        statuses: ["confirmed", "processing", "shipped", "delivered"],
      })
      .getRawOne();

    return {
      totalOrders,
      pendingOrders,
      completedOrders,
      totalRevenue: parseFloat(revenueResult?.totalRevenue || "0"),
    };
  }
}
