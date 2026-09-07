#ifndef RUNNER_FLUTTER_WINDOW_H_
#define RUNNER_FLUTTER_WINDOW_H_

#include <flutter/dart_project.h>
#include <flutter/flutter_view_controller.h>
#include <flutter/method_channel.h>
#include <flutter/standard_method_codec.h>

#include <memory>

#include "floating_ball_drop_target.h"
#include "popup_window_host.h"
#include "win32_window.h"

// A window that does nothing but host a Flutter view.
class FlutterWindow : public Win32Window {
 public:
  // Creates a new FlutterWindow hosting a Flutter view running |project|.
  explicit FlutterWindow(const flutter::DartProject& project);
  virtual ~FlutterWindow();

 protected:
  // Win32Window:
  bool OnCreate() override;
  void OnDestroy() override;
  LRESULT MessageHandler(HWND window, UINT const message, WPARAM const wparam,
                         LPARAM const lparam) noexcept override;

 private:
  // The project to run.
  flutter::DartProject project_;

  // The Flutter instance hosted by this window.
  std::unique_ptr<flutter::FlutterViewController> flutter_controller_;

  // Method channel for forwarding second-instance args to Dart.
  std::unique_ptr<flutter::MethodChannel<flutter::EncodableValue>>
      single_instance_channel_;

  // Floating ball: MethodChannel + OLE drop target (plan S1.2).
  std::unique_ptr<flutter::MethodChannel<flutter::EncodableValue>>
      floating_ball_channel_;
  FloatingBallDropTarget* ball_drop_target_ = nullptr;  // COM ref-counted

  // 外部唤起独立快速下载小窗宿主（第二 Flutter 引擎，懒创建常驻复用）。
  // 必须先于 flutter_controller_ 销毁（其 host_channel_ 引用主引擎 messenger）。
  std::unique_ptr<PopupWindowHost> popup_host_;

  // Tracks whether the window was hidden via ShowWindow(SW_HIDE) so that
  // we can synthesize a SIZE_RESTORED event when it becomes visible again.
  bool window_hidden_ = false;
};

#endif  // RUNNER_FLUTTER_WINDOW_H_
