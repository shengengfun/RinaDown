// 任务级代理选择模型 —— 纯 Dart,不依赖 bindings/rinf,popup 独立窗进程可安全 import。
//
// wire 语义(复用 CreateTask/BatchCreateTask 等信号的 proxyUrl 字段):
//   ''           = 跟随全局设置
//   'direct://'  = 强制直连(压过全局代理与 Auto)
//   'system://'  = 跟随系统代理(引擎启动时现场解析)
//   其他非空     = 自定义代理 URL

/// 强制直连哨兵值
const kProxyDirectSentinel = 'direct://';

/// 跟随系统代理哨兵值
const kProxySystemSentinel = 'system://';

/// 任务代理选择项
enum TaskProxyChoice {
  /// 跟随全局设置(proxyUrl = '')
  followGlobal,

  /// 强制直连(proxyUrl = 'direct://')
  direct,

  /// 跟随系统代理(proxyUrl = 'system://')
  system,

  /// 使用全局手动代理配置(proxyUrl = 全局手动代理 URL)
  globalManual,

  /// 自定义代理 URL(proxyUrl = 用户输入)
  custom,
}

/// 从 wire 值反推选择项。
///
/// [manualUrl] 为全局手动代理配置拼出的 URL(空表示未配置);
/// 当 [url] 与其相等时归类为 [TaskProxyChoice.globalManual]。
TaskProxyChoice choiceFromProxyUrl(String url, String manualUrl) {
  if (url.isEmpty) return TaskProxyChoice.followGlobal;
  if (url == kProxyDirectSentinel) return TaskProxyChoice.direct;
  if (url == kProxySystemSentinel) return TaskProxyChoice.system;
  if (manualUrl.isNotEmpty && url == manualUrl) {
    return TaskProxyChoice.globalManual;
  }
  return TaskProxyChoice.custom;
}

/// 从选择项生成 wire 值。
///
/// [manualUrl] 为全局手动代理 URL,[customUrl] 为用户输入的自定义 URL。
String proxyUrlFromChoice(
  TaskProxyChoice c,
  String manualUrl,
  String customUrl,
) {
  switch (c) {
    case TaskProxyChoice.followGlobal:
      return '';
    case TaskProxyChoice.direct:
      return kProxyDirectSentinel;
    case TaskProxyChoice.system:
      return kProxySystemSentinel;
    case TaskProxyChoice.globalManual:
      return manualUrl;
    case TaskProxyChoice.custom:
      return customUrl.trim();
  }
}
