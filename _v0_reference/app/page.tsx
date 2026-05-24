"use client"

import { useState, useEffect, useCallback } from "react"
import { RecordingPanel } from "@/components/recording-panel"
import { RecordingStatusBar } from "@/components/recording-status-bar"
import { PreviewView } from "@/components/preview-view"
import { AnimatePresence } from "framer-motion"

type AppState = "idle" | "recording" | "preview"

export default function LuZhiApp() {
  const [appState, setAppState] = useState<AppState>("idle")
  const [recordingMode, setRecordingMode] = useState<"fullscreen" | "window" | "area">("fullscreen")
  const [systemAudioEnabled, setSystemAudioEnabled] = useState(true)
  const [micEnabled, setMicEnabled] = useState(false)
  const [micVolume, setMicVolume] = useState(60)
  const [elapsedTime, setElapsedTime] = useState(0)
  const [isPaused, setIsPaused] = useState(false)

  // Simulate mic volume changes when mic is enabled
  useEffect(() => {
    if (!micEnabled) return
    const interval = setInterval(() => {
      setMicVolume(Math.floor(Math.random() * 60) + 20)
    }, 200)
    return () => clearInterval(interval)
  }, [micEnabled])

  // Timer for recording
  useEffect(() => {
    if (appState !== "recording" || isPaused) return
    const interval = setInterval(() => {
      setElapsedTime((prev) => prev + 1)
    }, 1000)
    return () => clearInterval(interval)
  }, [appState, isPaused])

  const handleStartRecording = useCallback(() => {
    setAppState("recording")
    setElapsedTime(0)
    setIsPaused(false)
  }, [])

  const handlePauseRecording = useCallback(() => {
    setIsPaused((prev) => !prev)
  }, [])

  const handleStopRecording = useCallback(() => {
    setAppState("preview")
  }, [])

  const handleBackToIdle = useCallback(() => {
    setAppState("idle")
    setElapsedTime(0)
    setIsPaused(false)
  }, [])

  // Idle state - Show recording panel
  if (appState === "idle") {
    return (
      <div className="min-h-screen flex items-center justify-center bg-background p-8">
        {/* Desktop app mock frame */}
        <div className="relative">
          {/* App background hint */}
          <div className="absolute inset-0 -z-10 rounded-3xl bg-gradient-to-br from-primary/5 via-transparent to-primary/5 blur-3xl scale-150" />
          
          <RecordingPanel
            recordingMode={recordingMode}
            setRecordingMode={setRecordingMode}
            systemAudioEnabled={systemAudioEnabled}
            setSystemAudioEnabled={setSystemAudioEnabled}
            micEnabled={micEnabled}
            setMicEnabled={setMicEnabled}
            micVolume={micVolume}
            onStartRecording={handleStartRecording}
          />
        </div>
      </div>
    )
  }

  // Recording state - Show mini status bar
  if (appState === "recording") {
    return (
      <div className="min-h-screen flex flex-col items-center justify-between bg-background p-8">
        {/* Top status bar */}
        <div className="pt-4">
          <AnimatePresence>
            <RecordingStatusBar
              elapsedTime={elapsedTime}
              isPaused={isPaused}
              onPause={handlePauseRecording}
              onStop={handleStopRecording}
            />
          </AnimatePresence>
        </div>

        {/* Simulated desktop area being recorded */}
        <div className="flex-1 w-full max-w-4xl mx-auto my-8 rounded-2xl border-2 border-dashed border-primary/20 flex items-center justify-center">
          <div className="text-center text-muted-foreground">
            <p className="text-sm mb-1">正在录制 {recordingMode === "fullscreen" ? "全屏" : recordingMode === "window" ? "窗口" : "区域"}</p>
            <p className="text-xs opacity-60">此区域表示被录制的屏幕内容</p>
          </div>
        </div>

        <div className="h-12" />
      </div>
    )
  }

  // Preview state - Show full preview & beautification view
  return <PreviewView onBack={handleBackToIdle} />
}
