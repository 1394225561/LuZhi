"use client"

import { Monitor, AppWindow, Square, Volume2, Mic, Circle } from "lucide-react"
import { Button } from "@/components/ui/button"
import { cn } from "@/lib/utils"
import { motion } from "framer-motion"

interface RecordingPanelProps {
  recordingMode: "fullscreen" | "window" | "area"
  setRecordingMode: (mode: "fullscreen" | "window" | "area") => void
  systemAudioEnabled: boolean
  setSystemAudioEnabled: (enabled: boolean) => void
  micEnabled: boolean
  setMicEnabled: (enabled: boolean) => void
  micVolume: number
  onStartRecording: () => void
}

export function RecordingPanel({
  recordingMode,
  setRecordingMode,
  systemAudioEnabled,
  setSystemAudioEnabled,
  micEnabled,
  setMicEnabled,
  micVolume,
  onStartRecording,
}: RecordingPanelProps) {
  const modes = [
    { id: "fullscreen" as const, icon: Monitor, label: "全屏" },
    { id: "window" as const, icon: AppWindow, label: "窗口" },
    { id: "area" as const, icon: Square, label: "区域" },
  ]

  return (
    <motion.div
      initial={{ opacity: 0, y: 20, scale: 0.95 }}
      animate={{ opacity: 1, y: 0, scale: 1 }}
      transition={{ duration: 0.3, ease: "easeOut" }}
      className="bg-card/95 backdrop-blur-xl border border-border/50 rounded-2xl p-5 shadow-2xl shadow-black/30 w-[320px]"
    >
      {/* Header */}
      <div className="flex items-center justify-between mb-5">
        <div className="flex items-center gap-2">
          <div className="w-8 h-8 rounded-lg bg-primary/20 flex items-center justify-center">
            <Circle className="w-4 h-4 text-primary" />
          </div>
          <span className="font-semibold text-foreground">录制</span>
        </div>
        <div className="flex items-center gap-1.5 text-xs text-muted-foreground font-mono">
          <span className="px-2 py-0.5 rounded bg-secondary">1080p</span>
          <span className="px-2 py-0.5 rounded bg-secondary">60fps</span>
        </div>
      </div>

      {/* Mode Selection */}
      <div className="mb-5">
        <p className="text-xs text-muted-foreground mb-2.5 uppercase tracking-wide">录制模式</p>
        <div className="flex gap-2">
          {modes.map((mode) => (
            <motion.button
              key={mode.id}
              whileHover={{ scale: 1.02 }}
              whileTap={{ scale: 0.98 }}
              onClick={() => setRecordingMode(mode.id)}
              className={cn(
                "flex-1 flex flex-col items-center gap-1.5 py-3 px-2 rounded-xl transition-all duration-200",
                recordingMode === mode.id
                  ? "bg-primary/15 border border-primary/40 text-primary"
                  : "bg-secondary/50 border border-transparent text-muted-foreground hover:bg-secondary hover:text-foreground"
              )}
            >
              <mode.icon className="w-5 h-5" />
              <span className="text-xs font-medium">{mode.label}</span>
            </motion.button>
          ))}
        </div>
      </div>

      {/* Audio Controls */}
      <div className="mb-6">
        <p className="text-xs text-muted-foreground mb-2.5 uppercase tracking-wide">音频设置</p>
        <div className="flex gap-2">
          {/* System Audio */}
          <motion.button
            whileHover={{ scale: 1.02 }}
            whileTap={{ scale: 0.98 }}
            onClick={() => setSystemAudioEnabled(!systemAudioEnabled)}
            className={cn(
              "flex-1 flex items-center justify-center gap-2 py-3 px-3 rounded-xl transition-all duration-200",
              systemAudioEnabled
                ? "bg-primary/15 border border-primary/40 text-primary"
                : "bg-secondary/50 border border-transparent text-muted-foreground hover:bg-secondary hover:text-foreground"
            )}
          >
            <Volume2 className="w-4 h-4" />
            <span className="text-xs font-medium">系统音频</span>
          </motion.button>

          {/* Microphone with volume indicator */}
          <motion.button
            whileHover={{ scale: 1.02 }}
            whileTap={{ scale: 0.98 }}
            onClick={() => setMicEnabled(!micEnabled)}
            className={cn(
              "flex-1 flex items-center justify-center gap-2 py-3 px-3 rounded-xl transition-all duration-200 relative overflow-hidden",
              micEnabled
                ? "bg-primary/15 border border-primary/40 text-primary"
                : "bg-secondary/50 border border-transparent text-muted-foreground hover:bg-secondary hover:text-foreground"
            )}
          >
            <Mic className="w-4 h-4" />
            <span className="text-xs font-medium">麦克风</span>
            {micEnabled && (
              <div className="absolute bottom-1 left-1/2 -translate-x-1/2 flex gap-0.5">
                {[...Array(5)].map((_, i) => (
                  <motion.div
                    key={i}
                    initial={{ height: 2 }}
                    animate={{ height: i < Math.ceil(micVolume / 20) ? 6 + i * 2 : 2 }}
                    transition={{ duration: 0.1 }}
                    className="w-1 rounded-full bg-primary/60"
                  />
                ))}
              </div>
            )}
          </motion.button>
        </div>
      </div>

      {/* Start Recording Button */}
      <motion.div whileHover={{ scale: 1.01 }} whileTap={{ scale: 0.99 }}>
        <Button
          onClick={onStartRecording}
          className="w-full h-12 rounded-xl bg-primary hover:bg-primary/90 text-primary-foreground font-semibold text-base shadow-lg shadow-primary/25 transition-all duration-200"
        >
          <Circle className="w-4 h-4 mr-2 fill-current" />
          开始录制
        </Button>
      </motion.div>
    </motion.div>
  )
}
