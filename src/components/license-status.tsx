import type { LicenseStatus as LicenseStatusPayload } from '@/lib/tauri'
import { cn } from '@/lib/utils'

interface LicenseStatusProps {
  status: LicenseStatusPayload | null
}

export function LicenseStatus({ status }: LicenseStatusProps) {
  if (!status) return null

  const label = status.activated
    ? '已激活'
    : status.isExpired
      ? '试用已过期'
      : `试用剩余 ${status.trialDaysRemaining} 天`

  return (
    <div
      className={cn(
        'rounded-lg border px-3 py-2 text-xs',
        status.activated
          ? 'border-emerald-500/30 bg-emerald-500/10 text-emerald-300'
          : status.isExpired
            ? 'border-destructive/40 bg-destructive/10 text-destructive'
            : 'border-border/50 bg-secondary/30 text-muted-foreground',
      )}
    >
      {label}
    </div>
  )
}
