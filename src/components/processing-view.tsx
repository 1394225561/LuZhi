import { motion } from 'framer-motion'
import { Loader2 } from 'lucide-react'

interface ProcessingViewProps {
  message?: string
}

export function ProcessingView({ message = '正在处理录制内容...' }: ProcessingViewProps) {
  return (
    <motion.div
      initial={{ opacity: 0 }}
      animate={{ opacity: 1 }}
      transition={{ duration: 0.3 }}
      className="min-h-screen flex items-center justify-center bg-background" data-tauri-drag-region="deep"
    >
      <div className="flex flex-col items-center gap-4">
        <motion.div
          animate={{ rotate: 360 }}
          transition={{ duration: 1, repeat: Infinity, ease: 'linear' }}
        >
          <Loader2 className="w-8 h-8 text-muted-foreground" />
        </motion.div>
        <p className="text-sm text-muted-foreground">{message}</p>
      </div>
    </motion.div>
  )
}
