import type { LucideIcon } from "lucide-react";
import type { ReactNode } from "react";

interface ScreenHeaderProps {
  title: string;
  subtitle: string;
  actions?: ReactNode;
}

export function ScreenHeader({ title, subtitle, actions }: ScreenHeaderProps) {
  return (
    <div className="flex flex-wrap items-start justify-between gap-4 mb-6">
      <div className="min-w-[240px] flex-1">
        <h1 className="text-[22px] font-semibold leading-tight text-text-primary">
          {title}
        </h1>
        <p className="text-[13px] text-text-secondary mt-1">{subtitle}</p>
      </div>
      {actions && <div className="flex items-center gap-2 shrink-0">{actions}</div>}
    </div>
  );
}

interface EmptyStateCardProps {
  icon: LucideIcon;
  title: string;
  description: string;
  action?: ReactNode;
}

export function EmptyStateCard({
  icon: Icon,
  title,
  description,
  action,
}: EmptyStateCardProps) {
  return (
    <div className="bg-surface border border-border-subtle rounded-lg px-6 py-12 flex flex-col items-center text-center">
      <div className="w-11 h-11 rounded-full bg-subtle flex items-center justify-center text-text-secondary mb-3.5">
        <Icon className="w-5 h-5" strokeWidth={1.5} />
      </div>
      <h2 className="text-[14px] font-semibold text-text-primary">{title}</h2>
      <p className="text-[13px] text-text-secondary mt-1 max-w-[480px] leading-relaxed">
        {description}
      </p>
      {action && <div className="mt-5">{action}</div>}
    </div>
  );
}
