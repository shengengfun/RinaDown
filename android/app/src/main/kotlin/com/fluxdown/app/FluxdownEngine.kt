package com.fluxdown.app

import io.flutter.embedding.engine.FlutterEngine
import io.flutter.embedding.engine.FlutterEngineCache

/**
 * FluxDown 单 FlutterEngine 保持者。
 *
 * FluxdownMainActivity 与 ExternalDownloadActivity 共享同一个引擎，保证单 Dart 会话、
 * 下载状态、Rust 桥与前台服务不重复初始化。首个启动的 Activity 走 FlutterActivity
 * 默认的 createFlutterEngine 路径，并在 onStart 缓存引擎；后续 Activity 通过
 * [io.flutter.embedding.android.FlutterActivity.getCachedEngineId] 复用。
 *
 * 两个宿主都必须让引擎独立于 Activity 生命周期，并在 Flutter embedding 将引擎
 * 驱逐给另一个宿主时结束自身。否则首个宿主会销毁缓存引擎，或留下一个已经与
 * FlutterView 解绑、后续只能显示黑屏的 Activity。
 */
object FluxdownEngine {
    const val ENGINE_ID = "com.fluxdown.app/engine"

    val cached: FlutterEngine?
        get() = FlutterEngineCache.getInstance().get(ENGINE_ID)

    fun cacheIfAbsent(engine: FlutterEngine) {
        if (cached == null) {
            FlutterEngineCache.getInstance().put(ENGINE_ID, engine)
        }
    }
}