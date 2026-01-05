/**
 * API Client for React Native
 *
 * This client talks directly to the local NestJS server in development.
 * The NestJS server uses HTTP_PROXY to route K8s service calls through Roxy.
 *
 * Architecture:
 *   React Native App → localhost:3000 (NestJS)
 *   NestJS → HTTP_PROXY (Roxy) → K8s services
 */

import axios, { AxiosInstance } from "axios";
import Constants from "expo-constants";

// In development, talk directly to local NestJS server
// In production, this would be your actual API URL
const API_BASE_URL = __DEV__
  ? "http://localhost:3000"
  : Constants.expoConfig?.extra?.apiBaseUrl || "https://api.example.app";

/**
 * Order item
 */
export interface OrderItem {
  productId: string;
  productName: string;
  productSku?: string;
  quantity: number;
  unitPrice: number;
  discount?: number;
}

/**
 * Address
 */
export interface Address {
  street: string;
  city: string;
  state: string;
  postalCode: string;
  country: string;
}

/**
 * Order
 */
export interface Order {
  id: string;
  customerId: string;
  customerEmail: string;
  status: "pending" | "confirmed" | "processing" | "shipped" | "delivered" | "cancelled";
  items: OrderItem[];
  subtotal: number;
  tax: number;
  total: number;
  currency: string;
  shippingAddress?: Address;
  billingAddress?: Address;
  notes?: string;
  createdAt: string;
  updatedAt: string;
}

/**
 * Create order request
 */
export interface CreateOrderRequest {
  customerId: string;
  customerEmail: string;
  items: OrderItem[];
  shippingAddress?: Address;
  billingAddress?: Address;
  notes?: string;
  currency?: string;
}

/**
 * Order statistics
 */
export interface OrderStats {
  totalOrders: number;
  pendingOrders: number;
  completedOrders: number;
  totalRevenue: number;
}

/**
 * Dependency health status
 */
export interface DependencyHealth {
  database: boolean;
  kafka: boolean;
  configService: boolean;
  message: string;
}

/**
 * Transform order response to ensure numeric fields are numbers.
 * TypeORM returns decimal columns as strings from PostgreSQL.
 */
function transformOrder(order: any): Order {
  return {
    ...order,
    subtotal: parseFloat(order.subtotal) || 0,
    tax: parseFloat(order.tax) || 0,
    total: parseFloat(order.total) || 0,
    items:
      order.items?.map((item: any) => ({
        ...item,
        unitPrice: parseFloat(item.unitPrice) || 0,
        discount: item.discount ? parseFloat(item.discount) : undefined,
      })) || [],
  };
}

/**
 * API Client
 */
class ApiClient {
  private client: AxiosInstance;

  constructor() {
    this.client = axios.create({
      baseURL: API_BASE_URL,
      timeout: 30000,
      headers: {
        "Content-Type": "application/json",
      },
    });

    console.log(`[API] Base URL: ${API_BASE_URL}`);
  }

  // Orders

  async createOrder(order: CreateOrderRequest): Promise<Order> {
    const response = await this.client.post<Order>("/api/orders", order);
    return transformOrder(response.data);
  }

  async getOrders(params?: {
    status?: string;
    customerId?: string;
    limit?: number;
    offset?: number;
  }): Promise<{ orders: Order[]; total: number }> {
    const response = await this.client.get<{ orders: Order[]; total: number }>("/api/orders", { params });
    return {
      ...response.data,
      orders: response.data.orders.map(transformOrder),
    };
  }

  async getOrder(orderId: string): Promise<Order> {
    const response = await this.client.get<Order>(`/api/orders/${orderId}`);
    return transformOrder(response.data);
  }

  async confirmOrder(orderId: string): Promise<Order> {
    const response = await this.client.post<Order>(`/api/orders/${orderId}/confirm`);
    return transformOrder(response.data);
  }

  async cancelOrder(orderId: string, reason?: string): Promise<Order> {
    const response = await this.client.post<Order>(`/api/orders/${orderId}/cancel`, { reason });
    return transformOrder(response.data);
  }

  async getOrderStats(): Promise<OrderStats> {
    const response = await this.client.get<OrderStats>("/api/orders/stats/summary");
    return response.data;
  }

  async checkDependencies(): Promise<DependencyHealth> {
    const response = await this.client.get<DependencyHealth>("/api/orders/health/dependencies");
    return response.data;
  }

  // Utility

  getConfig() {
    return {
      baseUrl: API_BASE_URL,
      isDev: __DEV__,
    };
  }
}

export const apiClient = new ApiClient();
