import { Pause, Square, Circle } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { motion } from 'framer-motion'

interface RecordingStatusBarProps {
  elapsedTime: number
  isPaused: boolean
  onPause: () => void
  onStop: () => void
}

function formatTime(seconds: number): string {
  const hrs = Math.floor(seconds / 3600)
  const mins = Math.floor((seconds % 3600) / 60)
  const secs = seconds % 60
  return `${hrs.toString().padStart(2, '0')}:${mins.toString().padStart(2, '0')}:${secs.toString().padStart(2, '0')}`
}

export function RecordingStatusBar({
  elapsedTime,
  isPaused,
  onPause,
  onStop,
}: RecordingStatusBarProps) {
  return (
    <motion.div
      initial={{ opacity: 0, y: -20, scale: 0.9 }}
      animate={{ opacity: 1, y: 0, scale: 1 }}
      exit={{ opacity: 0, y: -20, scale: 0.9 }}
      transition={{ duration: 0.25, ease: 'easeOut' }}
      className="bg-card/95 backdrop-blur-xl border border-border/50 rounded-full px-4 py-2 shadow-2xl shadow-black/30 flex items-center gap-4"
    >
      {/* Recording Indicator */}
      <div className="flex items-center gap-2">
        <motion.div
          animate={isPaused ? {} : { scale: [1, 1.2, 1] }}
          transition={{ duration: 1.5, repeat: Infinity, ease: 'easeInOut' }}
          className="relative"
        >
          <Circle
            className={`w-3 h-3 fill-current ${
              isPaused ? 'text-muted-foreground' : 'text-recording animate-pulse-recording'
            }`}
          />
          {!isPaused && (
            <motion.div
              initial={{ scale: 1, opacity: 0.6 }}
              animate={{ scale: 2, opacity: 0 }}
              transition={{ duration: 1.5, repeat: Infinity, ease: 'easeOut' }}
              className="absolute inset-0 rounded-full bg-recording"
            />
          )}
        </motion.div>
        <span className="text-xs text-muted-foreground uppercase tracking-wider">
          {isPaused ? '暂停' : '录制中'}
        </span>
      </div>

      {/* Timer */}
      <div className="font-mono text-lg font-semibold text-foreground tabular-nums min-w-[80px] text-center">
        {formatTime(elapsedTime)}
      </div>

      {/* Controls */}
      <div className="flex items-center gap-1.5">
        <motion.div whileHover={{ scale: 1.1 }} whileTap={{ scale: 0.95 }}>
          <Button
            variant="ghost"
            size="icon"
            onClick={onPause}
            className="h-8 w-8 rounded-full hover:bg-secondary"
          >
            {isPaused ? (
              <Circle className="w-4 h-4 fill-primary text-primary" />
            ) : (
              <Pause className="w-4 h-4 text-foreground" />
            )}
          </Button>
        </motion.div>
        <motion.div whileHover={{ scale: 1.1 }} whileTap={{ scale: 0.95 }}>
          <Button
            variant="ghost"
            size="icon"
            onClick={onStop}
            className="h-8 w-8 rounded-full hover:bg-destructive/20 text-destructive hover:text-destructive"
          >
            <Square className="w-4 h-4 fill-current" />
          </Button>
        </motion.div>
      </div>
    </motion.div>
  )
}
