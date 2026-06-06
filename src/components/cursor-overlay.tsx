import { useRef, useEffect, useCallback } from 'react'
import type { EffectTimeline, CursorFrame, CursorClickEffect } from '@/lib/tauri'

interface CursorOverlayProps {
  videoRef: React.RefObject<HTMLVideoElement | null>
  containerRef: React.RefObject<HTMLDivElement | null>
  effectTimeline: EffectTimeline | null
}

/**
 * Finds the cursor frame closest to the given timestamp using binary search.
 * Returns undefined if no frames exist or timeline is empty.
 */
function findCursorFrame(frames: CursorFrame[], timeNanos: number): CursorFrame | undefined {
  if (frames.length === 0) return undefined
  if (frames.length === 1) return frames[0]

  let lo = 0
  let hi = frames.length - 1
  while (lo < hi) {
    const mid = (lo + hi) >> 1
    if (frames[mid].timestamp.nanos < timeNanos) {
      lo = mid + 1
    } else {
      hi = mid
    }
  }

  // lo is the first frame with timestamp >= timeNanos.
  // Compare with the previous frame to find the closest.
  if (lo > 0) {
    const prev = frames[lo - 1]
    const curr = frames[lo]
    const prevDiff = timeNanos - prev.timestamp.nanos
    const currDiff = curr.timestamp.nanos - timeNanos
    return prevDiff <= currDiff ? prev : curr
  }
  return frames[lo]
}

/**
 * Finds click effects that are active at the given timestamp.
 */
function findActiveClickEffects(
  effects: CursorClickEffect[],
  timeNanos: number,
): CursorClickEffect[] {
  return effects.filter(
    (e) => timeNanos >= e.start.nanos && timeNanos <= e.end.nanos,
  )
}

const CURSOR_SIZE = 24

/**
 * Draws a cursor glyph on the canvas. Uses a simple shaped path for each
 * cursor kind rather than loading SVG assets (which are only available in
 * the Rust backend).
 */
function drawCursor(
  ctx: CanvasRenderingContext2D,
  x: number,
  y: number,
  scale: number,
  opacity: number,
  kind: string,
) {
  ctx.save()
  ctx.globalAlpha = opacity
  ctx.translate(x, y)

  const s = CURSOR_SIZE * scale

  switch (kind) {
    case 'hand': {
      // Hand cursor: open palm shape
      ctx.fillStyle = '#ffffff'
      ctx.strokeStyle = '#000000'
      ctx.lineWidth = 1.5
      ctx.beginPath()
      // Simplified hand: circle with a pointer finger
      ctx.arc(0, 0, s * 0.35, 0, Math.PI * 2)
      ctx.fill()
      ctx.stroke()
      // Pointer finger
      ctx.beginPath()
      ctx.moveTo(0, -s * 0.35)
      ctx.lineTo(s * 0.1, -s * 0.6)
      ctx.lineTo(-s * 0.1, -s * 0.6)
      ctx.closePath()
      ctx.fill()
      ctx.stroke()
      break
    }
    case 'iBeam': {
      // I-beam text cursor
      ctx.strokeStyle = '#ffffff'
      ctx.lineWidth = 2
      ctx.lineCap = 'round'
      const halfH = s * 0.4
      const halfW = s * 0.15
      ctx.beginPath()
      ctx.moveTo(-halfW, -halfH)
      ctx.lineTo(halfW, -halfH)
      ctx.moveTo(0, -halfH)
      ctx.lineTo(0, halfH)
      ctx.moveTo(-halfW, halfH)
      ctx.lineTo(halfW, halfH)
      ctx.stroke()
      // Shadow for visibility
      ctx.strokeStyle = '#000000'
      ctx.lineWidth = 1
      ctx.globalAlpha = opacity * 0.3
      ctx.beginPath()
      ctx.moveTo(-halfW - 1, -halfH - 1)
      ctx.lineTo(halfW - 1, -halfH - 1)
      ctx.moveTo(-1, -halfH - 1)
      ctx.lineTo(-1, halfH - 1)
      ctx.moveTo(-halfW - 1, halfH - 1)
      ctx.lineTo(halfW - 1, halfH - 1)
      ctx.stroke()
      break
    }
    default: {
      // Arrow cursor (default)
      ctx.fillStyle = '#ffffff'
      ctx.strokeStyle = '#000000'
      ctx.lineWidth = 1.5
      ctx.beginPath()
      ctx.moveTo(0, 0)
      ctx.lineTo(0, -s * 0.75)
      ctx.lineTo(s * 0.35, -s * 0.35)
      ctx.closePath()
      ctx.fill()
      ctx.stroke()
      // Tail
      ctx.beginPath()
      ctx.moveTo(0, 0)
      ctx.lineTo(-s * 0.15, s * 0.35)
      ctx.strokeStyle = '#000000'
      ctx.lineWidth = 2
      ctx.stroke()
      break
    }
  }

  ctx.restore()
}

/**
 * Draws a click magnification ring at the given position.
 */
function drawClickRing(
  ctx: CanvasRenderingContext2D,
  x: number,
  y: number,
  effect: CursorClickEffect,
  timeNanos: number,
) {
  const totalNanos = effect.end.nanos - effect.start.nanos
  if (totalNanos <= 0) return

  const progress = (timeNanos - effect.start.nanos) / totalNanos
  // Triangle wave: expand in first 35%, shrink in remaining 65%
  const expandPhase = progress < 0.35
  const localProgress = expandPhase ? progress / 0.35 : (1 - progress) / 0.65

  const maxRadius = CURSOR_SIZE * effect.maxScale
  const radius = maxRadius * Math.min(localProgress, 1.0)
  const alpha = expandPhase
    ? effect.peakOpacity * (localProgress * localProgress) // ease-in during expand
    : effect.peakOpacity * (1 - (1 - localProgress) * (1 - localProgress)) // ease-out during shrink

  if (radius <= 0 || alpha <= 0) return

  ctx.save()
  ctx.globalAlpha = alpha
  ctx.strokeStyle = '#4A9EFF'
  ctx.lineWidth = 2.5
  ctx.beginPath()
  ctx.arc(x, y, radius, 0, Math.PI * 2)
  ctx.stroke()
  ctx.restore()
}

/**
 * Computes the video display rect within the container, accounting for
 * `object-fit: contain` letterboxing/pillarboxing.
 */
function getVideoDisplayRect(
  containerWidth: number,
  containerHeight: number,
  videoWidth: number,
  videoHeight: number,
): { x: number; y: number; width: number; height: number } {
  if (videoWidth === 0 || videoHeight === 0) {
    return { x: 0, y: 0, width: containerWidth, height: containerHeight }
  }

  const containerAspect = containerWidth / containerHeight
  const videoAspect = videoWidth / videoHeight

  let displayWidth: number
  let displayHeight: number

  if (videoAspect > containerAspect) {
    // Video is wider → fit to container width, letterbox top/bottom
    displayWidth = containerWidth
    displayHeight = containerWidth / videoAspect
  } else {
    // Video is taller → fit to container height, letterbox left/right
    displayHeight = containerHeight
    displayWidth = containerHeight * videoAspect
  }

  return {
    x: (containerWidth - displayWidth) / 2,
    y: (containerHeight - displayHeight) / 2,
    width: displayWidth,
    height: displayHeight,
  }
}

/**
 * Overlay component that renders cursor effects on top of the video preview.
 * Syncs with video playback time and draws cursor position, shape, scale,
 * opacity, and click magnification rings.
 */
export function CursorOverlay({
  videoRef,
  containerRef,
  effectTimeline,
}: CursorOverlayProps) {
  const canvasRef = useRef<HTMLCanvasElement>(null)
  const rafRef = useRef<number>(0)

  const render = useCallback(() => {
    const canvas = canvasRef.current
    const video = videoRef.current
    const container = containerRef.current
    if (!canvas || !video || !container || !effectTimeline) return

    const ctx = canvas.getContext('2d')
    if (!ctx) return

    // Match canvas size to container
    const rect = container.getBoundingClientRect()
    const dpr = window.devicePixelRatio || 1
    const canvasWidth = rect.width
    const canvasHeight = rect.height

    if (canvas.width !== canvasWidth * dpr || canvas.height !== canvasHeight * dpr) {
      canvas.width = canvasWidth * dpr
      canvas.height = canvasHeight * dpr
      canvas.style.width = `${canvasWidth}px`
      canvas.style.height = `${canvasHeight}px`
    }

    ctx.setTransform(dpr, 0, 0, dpr, 0, 0)
    ctx.clearRect(0, 0, canvasWidth, canvasHeight)

    if (!effectTimeline.renderCursorOverlay || effectTimeline.frames.length === 0) {
      rafRef.current = requestAnimationFrame(render)
      return
    }

    // Current video time in nanoseconds
    const timeNanos = video.currentTime * 1_000_000_000

    // Find the cursor frame for current time
    const frame = findCursorFrame(effectTimeline.frames, timeNanos)
    if (!frame) {
      rafRef.current = requestAnimationFrame(render)
      return
    }

    // Map source coordinates to canvas coordinates
    const videoDisplay = getVideoDisplayRect(
      canvasWidth,
      canvasHeight,
      video.videoWidth,
      video.videoHeight,
    )
    const scaleX = videoDisplay.width / (video.videoWidth || 1)
    const scaleY = videoDisplay.height / (video.videoHeight || 1)
    const cursorX = videoDisplay.x + frame.x * scaleX
    const cursorY = videoDisplay.y + frame.y * scaleY

    // Draw click effects (behind cursor)
    const activeClicks = findActiveClickEffects(effectTimeline.clickEffects, timeNanos)
    for (const click of activeClicks) {
      const clickX = videoDisplay.x + click.x * scaleX
      const clickY = videoDisplay.y + click.y * scaleY
      drawClickRing(ctx, clickX, clickY, click, timeNanos)
    }

    // Draw cursor
    drawCursor(ctx, cursorX, cursorY, frame.scale, frame.opacity, frame.kind)

    rafRef.current = requestAnimationFrame(render)
  }, [videoRef, containerRef, effectTimeline])

  useEffect(() => {
    rafRef.current = requestAnimationFrame(render)
    return () => cancelAnimationFrame(rafRef.current)
  }, [render])

  if (!effectTimeline || !effectTimeline.renderCursorOverlay) {
    return null
  }

  return (
    <canvas
      ref={canvasRef}
      className="absolute inset-0 pointer-events-none"
      style={{ zIndex: 10 }}
    />
  )
}
