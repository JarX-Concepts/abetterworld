package com.jarxconcepts.abetterworld

import android.view.Surface

object Renderer {
    init {
        System.loadLibrary("abetterworld_android")
    }

    external fun nativeCreateState(): Long
    external fun nativeDestroyState(statePtr: Long)
    external fun nativeInitRenderer(statePtr: Long, surface: Surface, width: Int, height: Int)
    external fun nativeResize(statePtr: Long, width: Int, height: Int)
    external fun nativeRender(statePtr: Long)
}
