// Renders the widget summary panel and its retry action
export interface WidgetPanelProps {
  title: string;
  onRetry?: () => void;
}

export function WidgetPanel({ title, onRetry }: WidgetPanelProps) {
  return title;
}
