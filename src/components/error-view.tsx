import { motion } from 'framer-motion'
import { AlertCircle, RotateCcw } from 'lucide-react'
import { Button } from '@/components/ui/button'

interface ErrorViewProps {
  message: string
  onRetry?: () => void
  onBack?: () => void
}

export function ErrorView({ message, onRetry, onBack }: ErrorViewProps) {
  return (
    <motion.div
      initial={{ opacity: 0 }}
      animate={{ opacity: 1 }}
      transition={{ duration: 0.3 }}
      className="min-h-screen flex items-center justify-center bg-background" data-tauri-drag-region="deep"
    >
      <div className="flex flex-col items-center gap-4 max-w-sm text-center">
        <AlertCircle className="w-10 h-10 text-destructive" />
        <h2 className="text-lg font-semibold text-foreground">录制失败</h2>
        <p className="text-sm text-muted-foreground">{message}</p>
        <div className="flex gap-3 mt-2" data-tauri-drag-region={false}>
          {onRetry && (
            <Button variant="outline" onClick={onRetry}>
              <RotateCcw className="w-4 h-4 mr-2" />
              重试
            </Button>
          )}
          {onBack && (
            <Button variant="ghost" onClick={onBack}>
              返回
            </Button>
          )}
        </div>
      </div>
    </motion.div>
  )
}
