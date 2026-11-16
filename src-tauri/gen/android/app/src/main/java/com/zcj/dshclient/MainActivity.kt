package com.zcj.dshclient

import android.os.Bundle
import androidx.activity.enableEdgeToEdge

class MainActivity : TauriActivity() {
  override fun onCreate(savedInstanceState: Bundle?) {
    enableEdgeToEdge()
    PendingProfiles.cleanup(this)
    super.onCreate(savedInstanceState)
  }
}
