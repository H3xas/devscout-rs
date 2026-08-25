import type { NotificationEvent } from './types';

function groupByDay(events: NotificationEvent[]): Map<string, NotificationEvent[]> {
  const byDay = new Map<string, NotificationEvent[]>();
  for (const event of events) {
    const day = event.timestamp.slice(0, 10);
    const bucket = byDay.get(day) ?? [];
    bucket.push(event);
    byDay.set(day, bucket);
  }
  return byDay;
}

function summarize(events: NotificationEvent[]): string {
  return `${events.length} notifications`;
}

export class NotificationDigestBuilder {
  private events: NotificationEvent[] = [];

  addEvent(event: NotificationEvent): void {
    this.events.push(event);
  }

  build(): string {
    const grouped = groupByDay(this.events);
    return Array.from(grouped.values()).map(summarize).join('; ');
  }
}

export function buildDailyDigest(events: NotificationEvent[]): string {
  return events.length ? summarize(events) : '';
}
