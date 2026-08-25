import type { HttpClient } from './http-client';
import type { Order } from './types';

export interface IOrderService {
  getOrder(id: string): Promise<Order>;
}

export class OrderService implements IOrderService {
  constructor(private readonly httpClient: HttpClient) {}

  async getOrder(id: string): Promise<Order> {
    return this.httpClient.get(`/orders/${id}`);
  }

  async cancelOrder(id: string): Promise<void> {
    await this.httpClient.post(`/orders/${id}/cancel`, {});
  }

  private validate(id: string): boolean {
    return id.length > 0;
  }
}
