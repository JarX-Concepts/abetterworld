package com.example.abetterworld

object Renderer {
    init {
        System.loadLibrary("abetterworld_android")
    }

    external fun nativeCreateState(): Long
    external fun nativeDestroyState(ptr: Long)
    external fun nativeInitRenderer(ptr: Long, width: Int, height: Int)
    external fun nativeVersion(): String
}
