package com.fluxdown.app

import android.app.Activity
import android.content.Intent
import android.os.Bundle

/**
 * Android 公开入口兼容路由。
 *
 * 一些浏览器会显式启动固定类名 `com.fluxdown.app.MainActivity`，因此该类必须保持
 * 稳定。下载 intent 转交给透明的 [ExternalDownloadActivity]；普通启动转交给真正的
 * Flutter 主界面 [FluxdownMainActivity]。路由本身不创建 FlutterEngine。
 */
class MainActivity : Activity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        forward(intent)
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        setIntent(intent)
        forward(intent)
    }

    private fun forward(source: Intent) {
        val target = when (source.action) {
            Intent.ACTION_SEND,
            Intent.ACTION_SEND_MULTIPLE,
            Intent.ACTION_VIEW,
            -> ExternalDownloadActivity::class.java

            else -> FluxdownMainActivity::class.java
        }
        startActivity(Intent(source).setClass(this, target))
        finish()
    }
}
