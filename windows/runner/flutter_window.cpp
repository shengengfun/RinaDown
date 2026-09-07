#include "flutter_window.h"

#include <windowsx.h>

#include <algorithm>
#include <optional>
#include <string>
#include <vector>

#include "flutter/generated_plugin_registrant.h"
#include "utils.h"

// Must match kCopyDataId in main.cpp.
static const ULONG_PTR kCopyDataId = 0x464C5558; // "FLUX"

namespace {

// Property name storing the original WndProc of the Flutter view child
// window (set when subclassing for top-edge hit-test forwarding).
constexpr const wchar_t kChildOriginalProcProp[] = L"FluxDownChildProc";

// True while the custom frame is active: WS_THICKFRAME present, not
// maximized (fullscreen strips the resize frame).
bool IsCustomFrameNormalState(HWND hwnd) {
  const LONG_PTR style = GetWindowLongPtr(hwnd, GWL_STYLE);
  return (style & WS_THICKFRAME) != 0 && !IsZoomed(hwnd);
}

// Resize band thickness in physical pixels for |hwnd|'s DPI.
int ResizeInsetFor(HWND hwnd) {
  const UINT dpi = GetDpiForWindow(hwnd);
  return GetSystemMetricsForDpi(SM_CXSIZEFRAME, dpi) +
         GetSystemMetricsForDpi(SM_CXPADDEDBORDER, dpi);
}

// Hit-tests |screen_pt| against the TOP resize band of |root|（左/右/下
// 是真正的非客户区框架，由系统处理；仅顶部带被客户区回收，需手动报告）。
LRESULT HitTestTopEdge(HWND root, POINT screen_pt) {
  RECT rc;
  if (!GetWindowRect(root, &rc)) {
    return HTNOWHERE;
  }
  const int inset = ResizeInsetFor(root);
  if (screen_pt.y >= rc.top + inset) {
    return HTNOWHERE;
  }
  if (screen_pt.x < rc.left + inset) return HTTOPLEFT;
  if (screen_pt.x >= rc.right - inset) return HTTOPRIGHT;
  return HTTOP;
}

// Subclass proc for the Flutter view child window: makes the top resize
// band mouse-transparent so WM_NCHITTEST reaches the top-level window,
// which then reports HTTOP/HTTOPLEFT/HTTOPRIGHT for native resizing.
LRESULT CALLBACK ChildTopEdgeForwardProc(HWND hwnd, UINT message,
                                         WPARAM wparam, LPARAM lparam) {
  auto original =
      reinterpret_cast<WNDPROC>(GetProp(hwnd, kChildOriginalProcProp));
  if (message == WM_NCHITTEST) {
    HWND root = GetAncestor(hwnd, GA_ROOT);
    if (root && IsCustomFrameNormalState(root)) {
      const POINT pt{GET_X_LPARAM(lparam), GET_Y_LPARAM(lparam)};
      if (HitTestTopEdge(root, pt) != HTNOWHERE) {
        return HTTRANSPARENT;
      }
    }
  }
  if (message == WM_NCDESTROY) {
    RemoveProp(hwnd, kChildOriginalProcProp);
  }
  return original ? CallWindowProc(original, hwnd, message, wparam, lparam)
                  : DefWindowProc(hwnd, message, wparam, lparam);
}

}  // namespace

FlutterWindow::FlutterWindow(const flutter::DartProject& project)
    : project_(project) {}

FlutterWindow::~FlutterWindow() {}

bool FlutterWindow::OnCreate() {
  if (!Win32Window::OnCreate()) {
    return false;
  }

  RECT frame = GetClientArea();

  // The size here must match the window dimensions to avoid unnecessary surface
  // creation / destruction in the startup path.
  flutter_controller_ = std::make_unique<flutter::FlutterViewController>(
      frame.right - frame.left, frame.bottom - frame.top, project_);
  // Ensure that basic setup of the controller was successful.
  if (!flutter_controller_->engine() || !flutter_controller_->view()) {
    return false;
  }
  RegisterPlugins(flutter_controller_->engine());

  // Create MethodChannel for forwarding second-instance args to Dart.
  single_instance_channel_ =
      std::make_unique<flutter::MethodChannel<flutter::EncodableValue>>(
          flutter_controller_->engine()->messenger(),
          "com.fluxdown/single_instance",
          &flutter::StandardMethodCodec::GetInstance());

  // Floating ball channel (plan A6): handles registerDropTarget /
  // unregisterDropTarget from Dart; forwards drop payloads back.
  floating_ball_channel_ =
      std::make_unique<flutter::MethodChannel<flutter::EncodableValue>>(
          flutter_controller_->engine()->messenger(),
          "com.fluxdown/floating_ball",
          &flutter::StandardMethodCodec::GetInstance());
  floating_ball_channel_->SetMethodCallHandler(
      [this](const flutter::MethodCall<flutter::EncodableValue>& call,
             std::unique_ptr<flutter::MethodResult<flutter::EncodableValue>>
                 result) {
        if (call.method_name() == "registerDropTarget") {
          const auto* args =
              std::get_if<flutter::EncodableMap>(call.arguments());
          if (!args) {
            result->Error("bad_args", "expected map with hwnd");
            return;
          }
          auto it = args->find(flutter::EncodableValue("hwnd"));
          if (it == args->end()) {
            result->Error("bad_args", "missing hwnd");
            return;
          }
          const int64_t hwnd_val =
              std::holds_alternative<int64_t>(it->second)
                  ? std::get<int64_t>(it->second)
                  : static_cast<int64_t>(std::get<int32_t>(it->second));
          if (!ball_drop_target_) {
            ball_drop_target_ =
                new FloatingBallDropTarget(floating_ball_channel_.get());
          }
          HRESULT hr = ball_drop_target_->RegisterOn(
              reinterpret_cast<HWND>(hwnd_val));
          if (SUCCEEDED(hr)) {
            result->Success();
          } else {
            result->Error("register_failed",
                          "RegisterDragDrop hr=" + std::to_string(hr));
          }
        } else if (call.method_name() == "unregisterDropTarget") {
          if (ball_drop_target_) {
            ball_drop_target_->Revoke();
          }
          result->Success();
        } else {
          result->NotImplemented();
        }
      });

  // 外部唤起独立快速下载小窗宿主 — 注册 fluxdown/popup_host 通道。
  // 弹窗窗口与第二引擎在首次 show 时才懒创建。
  popup_host_ = std::make_unique<PopupWindowHost>(
      flutter_controller_->engine()->messenger());

  SetChildContent(flutter_controller_->view()->GetNativeWindow());

  // 顶边缩放：客户区回收了顶部框架带（见 WM_NCCALCSIZE），子 Flutter 视图
  // 顶部 8px 命中带返回 HTTRANSPARENT，冒泡到顶层窗口报告 HTTOP。
  if (HWND view_hwnd = flutter_controller_->view()->GetNativeWindow()) {
    WNDPROC original = reinterpret_cast<WNDPROC>(SetWindowLongPtr(
        view_hwnd, GWLP_WNDPROC,
        reinterpret_cast<LONG_PTR>(ChildTopEdgeForwardProc)));
    SetProp(view_hwnd, kChildOriginalProcProp,
            reinterpret_cast<HANDLE>(original));
  }

  // Check --silentStart before the callback to avoid capturing by reference.
  const std::vector<std::string> cmd_args = GetCommandLineArguments();
  const bool is_silent_start =
      std::find(cmd_args.begin(), cmd_args.end(), "--silentStart") !=
      cmd_args.end();

  flutter_controller_->engine()->SetNextFrameCallback(
      [this, is_silent_start]() {
        // Skip showing the window on first frame if launched with --silentStart
        // (boot autostart silent mode).
        if (!is_silent_start) {
          this->Show();
        }
      });

  // Flutter can complete the first frame before the "show window" callback is
  // registered. The following call ensures a frame is pending to ensure the
  // window is shown. It is a no-op if the first frame hasn't completed yet.
  flutter_controller_->ForceRedraw();

  // UIPI hardening: allow a lower-integrity (Medium IL) second instance to
  // deliver its command-line args via WM_COPYDATA when this instance runs
  // elevated (High IL). Pairs with the single-instance mutex hardening in
  // main.cpp; without it the forwarded URL/torrent is silently dropped.
  if (HWND handle = GetHandle()) {
    ::ChangeWindowMessageFilterEx(handle, WM_COPYDATA, MSGFLT_ALLOW, nullptr);
  }

  return true;
}

void FlutterWindow::OnDestroy() {
  // 先销毁弹窗宿主：其主引擎通道引用 flutter_controller_ 的 messenger
  popup_host_ = nullptr;
  if (ball_drop_target_) {
    ball_drop_target_->Revoke();
    ball_drop_target_->Release();
    ball_drop_target_ = nullptr;
  }
  if (flutter_controller_) {
    flutter_controller_ = nullptr;
  }

  Win32Window::OnDestroy();
}

LRESULT
FlutterWindow::MessageHandler(HWND hwnd, UINT const message,
                              WPARAM const wparam,
                              LPARAM const lparam) noexcept {
  // Handle WM_SHOWWINDOW to ensure Flutter's rendering engine pauses when the
  // window is hidden to the system tray and resumes when shown again.
  //
  // window_manager.hide() calls ShowWindow(SW_HIDE) which sends only
  // WM_SHOWWINDOW(FALSE) — NOT WM_SIZE(SIZE_MINIMIZED).  Without
  // SIZE_MINIMIZED, Flutter's compositor does not pause vsync and continues
  // rendering at the monitor refresh rate (~60 fps), wasting 3-4 % CPU even
  // when there is nothing to draw.
  //
  // We synthesize the missing WM_SIZE messages so the Flutter engine always
  // receives the signal it needs to suspend/resume the rasterizer.
  //
  // Guard: lParam == 0 means the visibility change was triggered by a direct
  // ShowWindow call (our case).  Non-zero lParam values indicate parent-window
  // state changes (SW_PARENTCLOSING, SW_PARENTOPENING) — we skip those because
  // a real WM_SIZE(SIZE_MINIMIZED) was already dispatched by the minimize path.
  if (message == WM_SHOWWINDOW && lparam == 0 && flutter_controller_) {
    if (wparam == FALSE) {
      // Window is being hidden.  Tell Flutter to pause vsync.
      window_hidden_ = true;
      ::PostMessage(hwnd, WM_SIZE, SIZE_MINIMIZED, 0);
    } else if (wparam == TRUE && window_hidden_) {
      // Window is being shown after a SW_HIDE.  Tell Flutter to resume vsync
      // at the actual client dimensions (unchanged since we never minimized).
      window_hidden_ = false;
      RECT rect = GetClientArea();
      ::PostMessage(hwnd, WM_SIZE, SIZE_RESTORED,
                    MAKELPARAM(rect.right - rect.left,
                               rect.bottom - rect.top));
    }
    // Fall through — let the base handler propagate WM_SHOWWINDOW normally.
  }

  // Handle WM_COPYDATA from a second instance before Flutter processes it.
  if (message == WM_COPYDATA) {
    auto* cds = reinterpret_cast<COPYDATASTRUCT*>(lparam);
    if (cds && cds->dwData == kCopyDataId && single_instance_channel_) {
      // Reconstruct the argument list (newline-separated UTF-8).
      // Guard against cbData=0 cross-process case where lpData may be null.
      std::string payload;
      if (cds->cbData > 0 && cds->lpData != nullptr) {
        payload = std::string(static_cast<const char*>(cds->lpData),
                              cds->cbData);
      }
      flutter::EncodableList args_list;
      size_t start = 0;
      while (start < payload.size()) {
        size_t end = payload.find('\n', start);
        if (end == std::string::npos) end = payload.size();
        args_list.push_back(flutter::EncodableValue(payload.substr(start, end - start)));
        start = end + 1;
      }
      single_instance_channel_->InvokeMethod(
          "onSecondInstance",
          std::make_unique<flutter::EncodableValue>(args_list));
    }
    return 0;
  }

  // 窗口样式已去掉 WS_CAPTION（见 win32_window.cpp）。此处必须先于插件的
  // HandleTopLevelWindowProc 拦截 WM_NCCALCSIZE：window_manager 在
  // TitleBarStyle.hidden 下会自行改写 rgrc 抹掉侧边框架（黑边/无边框的
  // 根源）。交给 DefWindowProc 计算得到左/右/下标准框架后，把顶部框架带
  // 还给客户区，但**保留 1px 非客户区**——无标题栏窗口默认由 DWM 绘制
  // 顶边线，但只有当顶部仍存在非客户区带时 DWM 才会画满整条（含圆角）。
  // Win11 若把整条顶带（+0）都还给客户区，DWM 便不再绘制顶边线，导致
  // 只能靠 Dart 逐 widget 模拟补线（仅覆盖 HeaderBar 中段，侧边栏/右侧
  // 缺失）。故 Win10/Win11 一律保留 1px，让 DWM 画出全宽一致的顶边框。
  // 最大化/全屏仍交给插件调整（否则内容会被屏幕边缘裁掉）。
  if (message == WM_NCCALCSIZE && wparam == TRUE &&
      IsCustomFrameNormalState(hwnd)) {
    auto* params = reinterpret_cast<NCCALCSIZE_PARAMS*>(lparam);
    const LONG original_top = params->rgrc[0].top;
    DefWindowProc(hwnd, message, wparam, lparam);
    params->rgrc[0].top = original_top + 1;
    return 0;
  }

  // 最大化几何：窗口样式去掉了 WS_CAPTION（见 win32_window.cpp），系统
  // 预填的 MINMAXINFO 默认值随之退化为「整个显示器矩形」而非工作区，
  // 最大化后会盖住任务栏。window_manager 的 WM_GETMINMAXINFO 分支只写
  // Min/MaxTrackSize 且返回 0（短路 DefWindowProc），不会纠正这一点。
  // 故在转发给插件之前按窗口所在显示器的工作区改写 ptMaxSize /
  // ptMaxPosition —— 不 return，让插件继续叠加最小尺寸约束。
  // ptMaxPosition 是相对显示器左上角的偏移，不是屏幕绝对坐标。
  // 全屏由插件剥掉 WS_THICKFRAME 后直接 SetWindowPos 到显示器矩形，
  // 此处以 WS_THICKFRAME 为闸门跳过，避免把全屏夹回工作区。
  if (message == WM_GETMINMAXINFO &&
      (GetWindowLongPtr(hwnd, GWL_STYLE) & WS_THICKFRAME) != 0) {
    MONITORINFO mi = {};
    mi.cbSize = sizeof(mi);
    if (GetMonitorInfo(MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST),
                       &mi)) {
      auto* info = reinterpret_cast<MINMAXINFO*>(lparam);
      const LONG work_w = mi.rcWork.right - mi.rcWork.left;
      const LONG work_h = mi.rcWork.bottom - mi.rcWork.top;
      info->ptMaxPosition.x = mi.rcWork.left - mi.rcMonitor.left;
      info->ptMaxPosition.y = mi.rcWork.top - mi.rcMonitor.top;
      info->ptMaxSize.x = work_w;
      info->ptMaxSize.y = work_h;
      // 只放大不缩小：默认 MaxTrackSize 基于主显示器，副屏更大时会把
      // 最大化尺寸夹小；同时不主动限制用户手动拖拽的上限。
      info->ptMaxTrackSize.x = std::max(info->ptMaxTrackSize.x, work_w);
      info->ptMaxTrackSize.y = std::max(info->ptMaxTrackSize.y, work_h);
    }
  }

  // Give Flutter, including plugins, an opportunity to handle window messages.
  if (flutter_controller_) {
    std::optional<LRESULT> result =
        flutter_controller_->HandleTopLevelWindowProc(hwnd, message, wparam,
                                                      lparam);
    if (result) {
      return *result;
    }
  }

  switch (message) {
    case WM_FONTCHANGE:
      flutter_controller_->engine()->ReloadSystemFonts();
      break;
    case WM_NCHITTEST: {
      // 子视图顶部命中带返回 HTTRANSPARENT 后由此处接管：报告顶边缩放。
      // 左/右/下由 DefWindowProc 保留的原生框架处理。
      if (IsCustomFrameNormalState(hwnd)) {
        const POINT pt{GET_X_LPARAM(lparam), GET_Y_LPARAM(lparam)};
        const LRESULT hit = HitTestTopEdge(hwnd, pt);
        if (hit != HTNOWHERE) {
          return hit;
        }
      }
      break;
    }
  }

  return Win32Window::MessageHandler(hwnd, message, wparam, lparam);
}
