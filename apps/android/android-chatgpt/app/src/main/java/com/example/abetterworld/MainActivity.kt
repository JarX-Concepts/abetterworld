package com.example.abetterworld

import android.os.Bundle
import android.util.Log
import androidx.appcompat.app.AppCompatActivity

class MainActivity : AppCompatActivity() {
    private var nativePtr: Long = 0

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)

        nativePtr = Renderer.nativeCreateState()
        Renderer.nativeInitRenderer(nativePtr, 1080, 1920)

        val version = Renderer.nativeVersion()
        Log.d("A Better World", "Loaded version: $version")

        // simple UI
        setContentView(R.layout.activity_main)
    }

    override fun onDestroy() {
        Renderer.nativeDestroyState(nativePtr)
        super.onDestroy()
    }
}
