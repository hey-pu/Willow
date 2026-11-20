import type { LucideIcon } from "lucide-react";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { cn } from "@/lib/utils";

interface StatCardProps {
  title: string;
  value: string;
  exactValue?: string;
  sub?: string;
  icon: LucideIcon;
  tone?: "cost";
}

export function StatCard({ title, value, exactValue, sub, icon: Icon, tone }: StatCardProps) {
  const isCost = tone === "cost";
  return (
    <Card>
      <CardHeader className="flex-row items-center justify-between space-y-0">
        <CardTitle>{title}</CardTitle>
        <div className={cn("flex h-7 w-7 items-center justify-center rounded-lg", isCost ? "bg-cost/10 text-cost" : "bg-primary/10 text-primary")}>
          <Icon className="h-4 w-4" />
        </div>
      </CardHeader>
      <CardContent>
        <div title={exactValue} className={cn("text-2xl font-semibold tabular-nums", isCost && "text-cost")}>{value}</div>
        {sub && <p className="mt-1 text-xs text-muted-foreground">{sub}</p>}
      </CardContent>
    </Card>
  );
}
