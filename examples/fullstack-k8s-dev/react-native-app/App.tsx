/**
 * React Native Example App
 *
 * This app demonstrates using Roxy proxy for local development with Kubernetes.
 *
 * Setup:
 * 1. Start Roxy proxy (cargo run --release --bin roxy)
 * 2. Enable "System Proxy" toggle in Roxy's status bar (for iOS Simulator)
 * 3. Run this app with `npx expo start`
 *
 * Traffic flow:
 * - App → localhost:3000 (NestJS) → Roxy (HTTP_PROXY) → K8s services
 */

import React, { useEffect, useState, useCallback } from "react";
import {
  StyleSheet,
  View,
  Text,
  FlatList,
  TouchableOpacity,
  ActivityIndicator,
  RefreshControl,
  Alert,
  SafeAreaView,
  StatusBar,
} from "react-native";
import { apiClient, Order, OrderStats, DependencyHealth } from "./src/api/client";

// App theme colors (Catppuccin Mocha)
const colors = {
  base: "#1e1e2e",
  surface: "#313244",
  overlay: "#45475a",
  text: "#cdd6f4",
  subtext: "#a6adc8",
  blue: "#89b4fa",
  green: "#a6e3a1",
  red: "#f38ba8",
  yellow: "#f9e2af",
  peach: "#fab387",
};

// Status badge component
const StatusBadge: React.FC<{ status: Order["status"] }> = ({ status }) => {
  const statusColors: Record<Order["status"], string> = {
    pending: colors.yellow,
    confirmed: colors.blue,
    processing: colors.peach,
    shipped: colors.blue,
    delivered: colors.green,
    cancelled: colors.red,
  };

  return (
    <View style={[styles.statusBadge, { backgroundColor: statusColors[status] + "30" }]}>
      <Text style={[styles.statusText, { color: statusColors[status] }]}>
        {status.charAt(0).toUpperCase() + status.slice(1)}
      </Text>
    </View>
  );
};

// Order card component
const OrderCard: React.FC<{
  order: Order;
  onConfirm: () => void;
  onCancel: () => void;
}> = ({ order, onConfirm, onCancel }) => {
  return (
    <View style={styles.orderCard}>
      <View style={styles.orderHeader}>
        <Text style={styles.orderId}>Order #{order.id.slice(0, 8)}</Text>
        <StatusBadge status={order.status} />
      </View>
      <Text style={styles.orderEmail}>{order.customerEmail}</Text>
      <View style={styles.orderDetails}>
        <Text style={styles.orderItems}>{order.items.length} items</Text>
        <Text style={styles.orderTotal}>
          {order.currency} {order.total.toFixed(2)}
        </Text>
      </View>
      <Text style={styles.orderDate}>{new Date(order.createdAt).toLocaleDateString()}</Text>
      {order.status === "pending" && (
        <View style={styles.actionButtons}>
          <TouchableOpacity style={[styles.actionButton, styles.confirmButton]} onPress={onConfirm}>
            <Text style={styles.actionButtonText}>Confirm</Text>
          </TouchableOpacity>
          <TouchableOpacity style={[styles.actionButton, styles.cancelButton]} onPress={onCancel}>
            <Text style={styles.actionButtonText}>Cancel</Text>
          </TouchableOpacity>
        </View>
      )}
    </View>
  );
};

// Health indicator component
const HealthIndicator: React.FC<{ health: DependencyHealth | null; loading: boolean }> = ({
  health,
  loading,
}) => {
  if (loading) {
    return (
      <View style={styles.healthContainer}>
        <ActivityIndicator size="small" color={colors.blue} />
        <Text style={styles.healthText}>Checking K8s services...</Text>
      </View>
    );
  }

  if (!health) return null;

  const allHealthy = health.database && health.kafka && health.configService;

  return (
    <View style={styles.healthContainer}>
      <View style={[styles.healthDot, { backgroundColor: allHealthy ? colors.green : colors.red }]} />
      <Text style={styles.healthText}>
        K8s: {allHealthy ? "All Connected" : "Issues Detected"}
      </Text>
    </View>
  );
};

// Stats component
const StatsCard: React.FC<{ stats: OrderStats | null }> = ({ stats }) => {
  if (!stats) return null;

  return (
    <View style={styles.statsContainer}>
      <View style={styles.statItem}>
        <Text style={styles.statValue}>{stats.totalOrders}</Text>
        <Text style={styles.statLabel}>Total</Text>
      </View>
      <View style={styles.statItem}>
        <Text style={[styles.statValue, { color: colors.yellow }]}>{stats.pendingOrders}</Text>
        <Text style={styles.statLabel}>Pending</Text>
      </View>
      <View style={styles.statItem}>
        <Text style={[styles.statValue, { color: colors.green }]}>{stats.completedOrders}</Text>
        <Text style={styles.statLabel}>Done</Text>
      </View>
      <View style={styles.statItem}>
        <Text style={[styles.statValue, { color: colors.blue }]}>${stats.totalRevenue.toFixed(0)}</Text>
        <Text style={styles.statLabel}>Revenue</Text>
      </View>
    </View>
  );
};

// Main App
export default function App() {
  const [orders, setOrders] = useState<Order[]>([]);
  const [stats, setStats] = useState<OrderStats | null>(null);
  const [health, setHealth] = useState<DependencyHealth | null>(null);
  const [loading, setLoading] = useState(true);
  const [refreshing, setRefreshing] = useState(false);
  const [healthLoading, setHealthLoading] = useState(false);

  const fetchOrders = useCallback(async () => {
    try {
      const response = await apiClient.getOrders({ limit: 20 });
      setOrders(response.orders);
    } catch (error) {
      console.error("Failed to fetch orders:", error);
      Alert.alert("Error", "Failed to fetch orders. Is the NestJS server running?");
    }
  }, []);

  const fetchStats = useCallback(async () => {
    try {
      const response = await apiClient.getOrderStats();
      setStats(response);
    } catch (error) {
      console.error("Failed to fetch stats:", error);
    }
  }, []);

  const checkHealth = useCallback(async () => {
    setHealthLoading(true);
    try {
      const response = await apiClient.checkDependencies();
      setHealth(response);
    } catch (error) {
      console.error("Failed to check health:", error);
      setHealth({
        database: false,
        kafka: false,
        configService: false,
        message: "Health check failed",
      });
    } finally {
      setHealthLoading(false);
    }
  }, []);

  useEffect(() => {
    const loadData = async () => {
      setLoading(true);
      await Promise.all([fetchOrders(), fetchStats(), checkHealth()]);
      setLoading(false);
    };
    loadData();
  }, [fetchOrders, fetchStats, checkHealth]);

  const onRefresh = useCallback(async () => {
    setRefreshing(true);
    await Promise.all([fetchOrders(), fetchStats(), checkHealth()]);
    setRefreshing(false);
  }, [fetchOrders, fetchStats, checkHealth]);

  const createTestOrder = useCallback(async () => {
    try {
      const order = await apiClient.createOrder({
        customerId: `cust-${Date.now()}`,
        customerEmail: `test-${Date.now()}@example.com`,
        items: [
          { productId: "prod-001", productName: "Test Product", quantity: 2, unitPrice: 29.99 },
          { productId: "prod-002", productName: "Another Product", quantity: 1, unitPrice: 49.99 },
        ],
        shippingAddress: {
          street: "123 Test Street",
          city: "San Francisco",
          state: "CA",
          postalCode: "94102",
          country: "USA",
        },
        notes: "Created from React Native app via Roxy",
      });
      Alert.alert("Success", `Order ${order.id.slice(0, 8)} created!`);
      await Promise.all([fetchOrders(), fetchStats()]);
    } catch (error) {
      console.error("Failed to create order:", error);
      Alert.alert("Error", "Failed to create order");
    }
  }, [fetchOrders, fetchStats]);

  const confirmOrder = useCallback(
    async (orderId: string) => {
      try {
        await apiClient.confirmOrder(orderId);
        Alert.alert("Success", "Order confirmed!");
        await Promise.all([fetchOrders(), fetchStats()]);
      } catch (error) {
        console.error("Failed to confirm order:", error);
        Alert.alert("Error", "Failed to confirm order");
      }
    },
    [fetchOrders, fetchStats]
  );

  const cancelOrder = useCallback(
    async (orderId: string) => {
      Alert.alert("Cancel Order", "Are you sure?", [
        { text: "No", style: "cancel" },
        {
          text: "Yes",
          style: "destructive",
          onPress: async () => {
            try {
              await apiClient.cancelOrder(orderId, "Cancelled from app");
              Alert.alert("Success", "Order cancelled");
              await Promise.all([fetchOrders(), fetchStats()]);
            } catch (error) {
              console.error("Failed to cancel order:", error);
              Alert.alert("Error", "Failed to cancel order");
            }
          },
        },
      ]);
    },
    [fetchOrders, fetchStats]
  );

  if (loading) {
    return (
      <SafeAreaView style={styles.container}>
        <StatusBar barStyle="light-content" />
        <View style={styles.loadingContainer}>
          <ActivityIndicator size="large" color={colors.blue} />
          <Text style={styles.loadingText}>Connecting to API...</Text>
          <Text style={styles.loadingSubtext}>Make sure NestJS is running on localhost:3000</Text>
        </View>
      </SafeAreaView>
    );
  }

  return (
    <SafeAreaView style={styles.container}>
      <StatusBar barStyle="light-content" />

      <View style={styles.header}>
        <Text style={styles.title}>Orders</Text>
        <Text style={styles.subtitle}>NestJS → Roxy → K8s</Text>
        <HealthIndicator health={health} loading={healthLoading} />
      </View>

      <StatsCard stats={stats} />

      <TouchableOpacity style={styles.createButton} onPress={createTestOrder}>
        <Text style={styles.createButtonText}>+ Create Test Order</Text>
      </TouchableOpacity>

      <FlatList
        data={orders}
        keyExtractor={(item) => item.id}
        renderItem={({ item }) => (
          <OrderCard
            order={item}
            onConfirm={() => confirmOrder(item.id)}
            onCancel={() => cancelOrder(item.id)}
          />
        )}
        refreshControl={
          <RefreshControl refreshing={refreshing} onRefresh={onRefresh} tintColor={colors.blue} />
        }
        contentContainerStyle={styles.listContent}
        ListEmptyComponent={
          <View style={styles.emptyContainer}>
            <Text style={styles.emptyText}>No orders yet</Text>
            <Text style={styles.emptySubtext}>Create a test order to get started</Text>
          </View>
        }
      />

      <View style={styles.footer}>
        <Text style={styles.footerText}>API: {apiClient.getConfig().baseUrl}</Text>
      </View>
    </SafeAreaView>
  );
}

const styles = StyleSheet.create({
  container: {
    flex: 1,
    backgroundColor: colors.base,
  },
  loadingContainer: {
    flex: 1,
    justifyContent: "center",
    alignItems: "center",
    padding: 20,
  },
  loadingText: {
    color: colors.text,
    fontSize: 18,
    marginTop: 20,
  },
  loadingSubtext: {
    color: colors.subtext,
    fontSize: 14,
    marginTop: 10,
    textAlign: "center",
  },
  header: {
    padding: 20,
    paddingBottom: 10,
  },
  title: {
    fontSize: 32,
    fontWeight: "bold",
    color: colors.text,
  },
  subtitle: {
    fontSize: 14,
    color: colors.subtext,
    marginTop: 4,
  },
  healthContainer: {
    flexDirection: "row",
    alignItems: "center",
    marginTop: 10,
  },
  healthDot: {
    width: 8,
    height: 8,
    borderRadius: 4,
    marginRight: 8,
  },
  healthText: {
    color: colors.subtext,
    fontSize: 12,
  },
  statsContainer: {
    flexDirection: "row",
    backgroundColor: colors.surface,
    marginHorizontal: 20,
    borderRadius: 12,
    padding: 15,
    marginBottom: 15,
  },
  statItem: {
    flex: 1,
    alignItems: "center",
  },
  statValue: {
    fontSize: 24,
    fontWeight: "bold",
    color: colors.text,
  },
  statLabel: {
    fontSize: 12,
    color: colors.subtext,
    marginTop: 4,
  },
  createButton: {
    backgroundColor: colors.blue,
    marginHorizontal: 20,
    paddingVertical: 14,
    borderRadius: 12,
    alignItems: "center",
    marginBottom: 15,
  },
  createButtonText: {
    color: colors.base,
    fontSize: 16,
    fontWeight: "600",
  },
  listContent: {
    paddingHorizontal: 20,
    paddingBottom: 20,
  },
  orderCard: {
    backgroundColor: colors.surface,
    borderRadius: 12,
    padding: 15,
    marginBottom: 12,
  },
  orderHeader: {
    flexDirection: "row",
    justifyContent: "space-between",
    alignItems: "center",
    marginBottom: 8,
  },
  orderId: {
    fontSize: 16,
    fontWeight: "600",
    color: colors.text,
  },
  statusBadge: {
    paddingHorizontal: 10,
    paddingVertical: 4,
    borderRadius: 6,
  },
  statusText: {
    fontSize: 12,
    fontWeight: "600",
  },
  orderEmail: {
    color: colors.subtext,
    fontSize: 14,
    marginBottom: 8,
  },
  orderDetails: {
    flexDirection: "row",
    justifyContent: "space-between",
    marginBottom: 4,
  },
  orderItems: {
    color: colors.subtext,
    fontSize: 14,
  },
  orderTotal: {
    color: colors.text,
    fontSize: 16,
    fontWeight: "600",
  },
  orderDate: {
    color: colors.overlay,
    fontSize: 12,
    marginTop: 4,
  },
  actionButtons: {
    flexDirection: "row",
    marginTop: 12,
    gap: 10,
  },
  actionButton: {
    flex: 1,
    paddingVertical: 10,
    borderRadius: 8,
    alignItems: "center",
  },
  confirmButton: {
    backgroundColor: colors.green + "30",
  },
  cancelButton: {
    backgroundColor: colors.red + "30",
  },
  actionButtonText: {
    fontSize: 14,
    fontWeight: "600",
    color: colors.text,
  },
  emptyContainer: {
    alignItems: "center",
    paddingVertical: 40,
  },
  emptyText: {
    color: colors.text,
    fontSize: 18,
  },
  emptySubtext: {
    color: colors.subtext,
    fontSize: 14,
    marginTop: 8,
  },
  footer: {
    padding: 15,
    alignItems: "center",
    borderTopWidth: 1,
    borderTopColor: colors.surface,
  },
  footerText: {
    color: colors.overlay,
    fontSize: 11,
  },
});
