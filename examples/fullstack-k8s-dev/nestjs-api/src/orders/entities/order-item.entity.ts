import {
  Entity,
  PrimaryGeneratedColumn,
  Column,
  ManyToOne,
  JoinColumn,
} from "typeorm";
import { Order } from "./order.entity";

@Entity("order_items")
export class OrderItem {
  @PrimaryGeneratedColumn("uuid")
  id: string;

  @ManyToOne(() => Order, (order) => order.items, {
    onDelete: "CASCADE",
  })
  @JoinColumn({ name: "order_id" })
  order: Order;

  @Column({ name: "order_id" })
  orderId: string;

  @Column({ name: "product_id" })
  productId: string;

  @Column({ name: "product_name" })
  productName: string;

  @Column({ type: "varchar", name: "product_sku", nullable: true })
  productSku?: string;

  @Column({ type: "int", default: 1 })
  quantity: number;

  @Column({ type: "decimal", precision: 10, scale: 2, name: "unit_price" })
  unitPrice: number;

  @Column({ type: "decimal", precision: 10, scale: 2, default: 0 })
  discount: number;

  /**
   * Calculate the total price for this line item
   */
  get lineTotal(): number {
    return this.quantity * this.unitPrice - (this.discount || 0);
  }
}
