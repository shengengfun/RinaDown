import '../bindings/bindings.dart';

/// 把任务落盘的文件/文件夹放进**系统剪贴板**（不是复制路径文本）：复制完成后
/// 在资源管理器 / Finder / 文件管理器里 `Ctrl+V` 就能粘贴出一份拷贝。
///
/// 实现全在 Rust 端 `native/hub/src/clipboard_file.rs`：Windows 走 `CF_HDROP`，
/// macOS 走 osascript 写 «class furl»，Linux 借 `wl-copy` / `xclip`。文件与
/// 文件夹**共用同一条链路**——剪贴板里放的是路径引用，粘贴端按路径在磁盘上的
/// 真实类型决定复制单个文件还是整棵目录；返回的 [CopyPathToClipboardResult.isDir]
/// 只用于区分提示文案。
///
/// [path] 必须是落盘对象的绝对路径（[DownloadTask.filePath]——BT 全选多文件
/// 种子时它就是种子根目录）。
///
/// 结果经 `CopyPathToClipboardResult` 回传；5 秒无回执抛 `TimeoutException`。
/// 信号不带关联 id，短时间内连点两次会各自取到先到的那条回执——同一路径同一
/// 结果，不影响提示正确性。
Future<CopyPathToClipboardResult> copyPathToClipboard(String path) {
  // 先订阅再发信号，避免结果早于监听到达（同 RenameTaskResult 的用法）。
  final result = CopyPathToClipboardResult.rustSignalStream
      .map((pack) => pack.message)
      .first
      .timeout(const Duration(seconds: 5));
  CopyPathToClipboard(path: path).sendSignalToRust();
  return result;
}
