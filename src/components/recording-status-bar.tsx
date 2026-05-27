import { Mic, Pause, Square, Circle } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { motion } from 'framer-motion'

interface RecordingStatusBarProps {
  elapsedTime: number
  isPaused: boolean
  onPause: () => void
  onStop: () => void
  micEnabled: boolean
  micVolume: number
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
  micEnabled,
  micVolume,
}: RecordingStatusBarProps) {
  return (
    <motion.div
      initial={{ opacity: 0, y: -20, scale: 0.9 }}
      animate={{ opacity: 1, y: 0, scale: 1 }}
      exit={{ opacity: 0, y: -20, scale: 0.9 }}
      transition={{ duration: 0.25, ease: 'easeOut' }}
      className="bg-card/95 border border-border/50 rounded-full px-4 py-2 flex items-center gap-4"
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

      {/* Mic Level Indicator */}
      {micEnabled && (
        <div className="flex items-center gap-1.5" data-testid="mic-level-meter" data-level={micVolume}>
          <Mic className="w-3.5 h-3.5 text-muted-foreground" />
          <div className="flex gap-px items-end h-4">
            {[...Array(5)].map((_, i) => (
              <motion.div
                key={i}
                initial={{ height: 2 }}
                animate={{ height: i < Math.ceil(micVolume / 20) ? 4 + i * 2 : 2 }}
                transition={{ duration: 0.1 }}
                className="w-1 rounded-full bg-muted-foreground/60"
              />
            ))}
          </div>
        </div>
      )}

      {/* Controls */}
      <div className="flex items-center gap-1.5">
        <Button
          variant="ghost"
          size="icon"
          onClick={onPause}
          className="h-8 w-8 rounded-full hover:bg-secondary hover:scale-110 active:scale-95 transition-transform"
        >
          {isPaused ? (
            <Circle className="w-4 h-4 fill-primary text-primary" />
          ) : (
            <Pause className="w-4 h-4 text-foreground" />
          )}
        </Button>
        <Button
          variant="ghost"
          size="icon"
          onClick={onStop}
          className="h-8 w-8 rounded-full hover:bg-destructive/20 hover:scale-110 active:scale-95 transition-transform text-destructive hover:text-destructive"
        >
          <Square className="w-4 h-4 fill-current" />
        </Button>
      </div>
    </motion.div>
  )
}
