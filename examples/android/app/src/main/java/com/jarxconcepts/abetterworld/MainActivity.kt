// MainActivity.kt
package com.jarxconcepts.abetterworld

import android.os.Bundle
import android.view.Choreographer
import android.view.Surface
import android.view.SurfaceHolder
import android.view.SurfaceView
import androidx.appcompat.app.AppCompatActivity

class RenderLoop(
    private val statePtrProvider: () -> Long,
    private val renderFn: (Long, Long) -> Unit
) : Choreographer.FrameCallback {

    private var running = false

    fun start() {
        if (running) return
        running = true
        Choreographer.getInstance().postFrameCallback(this)
    }

    fun stop() {
        if (!running) return
        running = false
        Choreographer.getInstance().removeFrameCallback(this)
    }

    override fun doFrame(frameTimeNanos: Long) {
        if (!running) return
        val ptr = statePtrProvider()
        if (ptr != 0L) {
            renderFn(ptr, frameTimeNanos)
        }
        // schedule next frame at next vsync
        Choreographer.getInstance().postFrameCallback(this)
    }
}

class MainActivity : AppCompatActivity(), SurfaceHolder.Callback {
    private var nativePtr: Long = 0
    private lateinit var surfaceView: SurfaceView
    private lateinit var loop: RenderLoop
    private var surfaceReady = false

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)

        surfaceView = SurfaceView(this)
        setContentView(surfaceView)
        surfaceView.holder.addCallback(this)

        nativePtr = Renderer.nativeCreateState()
        loop = RenderLoop(
            statePtrProvider = { nativePtr },
            renderFn = { ptr, t -> Renderer.nativeRender(ptr) }
        )
    }

    override fun onDestroy() {
        loop.stop()
        if (nativePtr != 0L) {
            Renderer.nativeDestroyState(nativePtr)
            nativePtr = 0
        }
        super.onDestroy()
    }

    override fun onResume() {
        super.onResume()
        if (surfaceReady) loop.start()
    }

    override fun onPause() {
        // Pause rendering when app not visible
        loop.stop()
        super.onPause()
    }

    override fun surfaceCreated(holder: SurfaceHolder) {
        val w = maxOf(surfaceView.width, 1)
        val h = maxOf(surfaceView.height, 1)
        Renderer.nativeInitRenderer(nativePtr, holder.surface, w, h)
        surfaceReady = true
        loop.start()
    }

    override fun surfaceChanged(holder: SurfaceHolder, format: Int, width: Int, height: Int) {
        //Renderer.nativeResize(nativePtr, maxOf(width, 1), maxOf(height, 1))
    }

    override fun surfaceDestroyed(holder: SurfaceHolder) {
        surfaceReady = false
        loop.stop()
        // If you recreate the surface later, we'll re-init & restart in surfaceCreated
    }
}