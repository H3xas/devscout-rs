import { useEffect, useState } from 'react';

interface WidgetData {
  id: string;
  value: number;
}

export function useWidgetData(widgetId: string) {
  const [data, setData] = useState<WidgetData | null>(null);

  useEffect(() => {
    let cancelled = false;
    fetchWidget(widgetId).then((result) => {
      if (!cancelled) setData(result);
    });
    return () => {
      cancelled = true;
    };
  }, [widgetId]);

  return data;
}

async function fetchWidget(widgetId: string): Promise<WidgetData> {
  const response = await fetch(`/api/widgets/${widgetId}`);
  return response.json();
}
