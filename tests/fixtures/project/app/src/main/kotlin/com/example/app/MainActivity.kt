package com.example.app

import android.app.Activity
import android.os.Bundle

// Stub source for resolver tests. Resolution only checks that this file exists at
// app/src/main/kotlin/com/example/app/MainActivity.kt; the frame's line number is used
// verbatim in the emitted clickable token.
class MainActivity : Activity() {
    private lateinit var db: AppDatabase

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        load()
    }

    private fun load() {
        db.open()
    }
}
