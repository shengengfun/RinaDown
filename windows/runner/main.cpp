#include <flutter/dart_project.h>
#include <flutter/flutter_view_controller.h>
#include <windows.h>
#include <dbghelp.h>
#include <sddl.h>

#include <iostream>
#include <sstream>
#include <string>

#include "flutter_window.h"
#include "utils.h"

#pragma comment(lib, "dbghelp.lib")
#pragma comment(lib, "advapi32.lib")

static LONG WINAPI FluxDownCrashHandler(EXCEPTION_POINTERS* ep) {
  std::ostringstream oss;
  oss << "[CRASH_HANDLER] Unhandled exception! Code=0x"
      << std::hex << ep->ExceptionRecord->ExceptionCode
      << ", Address=0x" << ep->ExceptionRecord->ExceptionAddress
      << std::dec << "\n";
  OutputDebugStringA(oss.str().c_str());
  std::cerr << oss.str();
  std::cerr.flush();
  return EXCEPTION_CONTINUE_SEARCH;
}

// Unique mutex name for single-instance enforcement.
static const wchar_t kMutexName[] = L"Global\\FluxDown_SingleInstance_Mutex";
// Window class name used by Flutter runner (must match win32_window.cpp).
static const wchar_t kFlutterWindowClass[] = L"FLUTTER_RUNNER_WIN32_WINDOW";
// Window title (must match CreateCentered call below).
static const wchar_t kWindowTitle[] = L"FluxDown";
// Magic identifier for WM_COPYDATA to distinguish our messages.
static const ULONG_PTR kCopyDataId = 0x464C5558; // "FLUX" in hex

// Build a single UTF-8 string from command-line arguments, separated by '\n'.
static std::string JoinArguments(const std::vector<std::string>& args) {
  std::string result;
  for (size_t i = 0; i < args.size(); ++i) {
    if (i > 0) result += '\n';
    result += args[i];
  }
  return result;
}

// Try to find the existing FluxDown window, send it our command-line args
// via WM_COPYDATA, and bring it to the foreground.
static bool SendArgsToExistingInstance(const std::vector<std::string>& args) {
  HWND existing = ::FindWindow(kFlutterWindowClass, kWindowTitle);
  if (!existing) return false;

  // Send command-line arguments via WM_COPYDATA.
  std::string payload = JoinArguments(args);
  COPYDATASTRUCT cds = {};
  cds.dwData = kCopyDataId;
  cds.cbData = static_cast<DWORD>(payload.size());
  cds.lpData = const_cast<char*>(payload.data());
  ::SendMessage(existing, WM_COPYDATA, 0, reinterpret_cast<LPARAM>(&cds));

  // Bring existing window to foreground.
  // Handle all possible window states:
  //   - Minimized (IsIconic)  → SW_RESTORE
  //   - Hidden to tray (SW_HIDE, !IsWindowVisible) → SW_SHOW
  //   - Normal/visible        → just SetForegroundWindow
  if (::IsIconic(existing)) {
    ::ShowWindow(existing, SW_RESTORE);
  } else if (!::IsWindowVisible(existing)) {
    // Window is hidden (tray mode via window_manager.hide()).
    // ShowWindow(SW_SHOW) makes it visible; window_manager.show() called
    // via the WM_COPYDATA → Dart channel will also run and sync state.
    ::ShowWindow(existing, SW_SHOW);
  }
  ::SetForegroundWindow(existing);

  return true;
}

// Build a SECURITY_ATTRIBUTES that grants Everyone full access to the
// single-instance mutex and stamps it with a Low mandatory integrity label
// (no-write-up). Without this, a Medium-IL second instance cannot open the
// mutex a High-IL (elevated) first instance created: CreateMutex then returns
// ACCESS_DENIED instead of ERROR_ALREADY_EXISTS and the app double-launches.
// Returns nullptr (default security) on failure; on success the caller must
// LocalFree(*out_sd) after CreateMutex.
static LPSECURITY_ATTRIBUTES BuildCrossIntegritySA(SECURITY_ATTRIBUTES* sa,
                                                   PSECURITY_DESCRIPTOR* out_sd) {
  *out_sd = nullptr;
  PSECURITY_DESCRIPTOR sd = nullptr;
  // D: Everyone (WD) mutex all-access (0x1F0001). S: Low mandatory label,
  // no-write-up so equal/higher IL subjects may still open it.
  if (!::ConvertStringSecurityDescriptorToSecurityDescriptorW(
          L"D:(A;;0x1F0001;;;WD)S:(ML;;NW;;;LW)", SDDL_REVISION_1, &sd,
          nullptr)) {
    return nullptr;
  }
  sa->nLength = sizeof(*sa);
  sa->lpSecurityDescriptor = sd;
  sa->bInheritHandle = FALSE;
  *out_sd = sd;
  return sa;
}

int APIENTRY wWinMain(_In_ HINSTANCE instance, _In_opt_ HINSTANCE prev,
                      _In_ wchar_t *command_line, _In_ int show_command) {
  // Collect command-line arguments early (needed for both paths).
  std::vector<std::string> command_line_arguments = GetCommandLineArguments();

  ::SetUnhandledExceptionFilter(FluxDownCrashHandler);

  // --- Single-instance check ---
  // Try to create a named mutex. If it already exists, another instance
  // is running -- forward our args to it and exit.
  SECURITY_ATTRIBUTES mutex_sa = {};
  PSECURITY_DESCRIPTOR mutex_sd = nullptr;
  LPSECURITY_ATTRIBUTES mutex_psa = BuildCrossIntegritySA(&mutex_sa, &mutex_sd);
  HANDLE mutex = ::CreateMutex(mutex_psa, FALSE, kMutexName);
  const DWORD mutex_err = ::GetLastError();  // capture before LocalFree
  if (mutex_sd) ::LocalFree(mutex_sd);
  if (mutex && mutex_err == ERROR_ALREADY_EXISTS) {
    SendArgsToExistingInstance(command_line_arguments);
    ::CloseHandle(mutex);
    return EXIT_SUCCESS;
  }

  // Attach to console when present (e.g., 'flutter run') or create a
  // new console when running with a debugger.
  if (!::AttachConsole(ATTACH_PARENT_PROCESS) && ::IsDebuggerPresent()) {
    CreateAndAttachConsole();
  }

  // Initialize OLE (superset of COM STA init) — required by RegisterDragDrop
  // for the floating-ball drop target (S0.6). OleInitialize internally calls
  // CoInitializeEx(COINIT_APARTMENTTHREADED); same-thread re-entry from
  // plugins (e.g. window_manager's CoInitialize) returns S_FALSE, harmless.
  ::OleInitialize(nullptr);

  flutter::DartProject project(L"data");

  project.set_dart_entrypoint_arguments(std::move(command_line_arguments));

  FlutterWindow window(project);
  Win32Window::Size size(1280, 720);
  if (!window.CreateCentered(kWindowTitle, size)) {
    if (mutex) ::CloseHandle(mutex);
    return EXIT_FAILURE;
  }
  window.SetQuitOnClose(true);

  ::MSG msg;
  while (::GetMessage(&msg, nullptr, 0, 0)) {
    ::TranslateMessage(&msg);
    ::DispatchMessage(&msg);
  }

  {
    std::ostringstream oss;
    oss << "[MAIN] Message loop exited normally, WM_QUIT received. msg.wParam="
        << msg.wParam << "\n";
    OutputDebugStringA(oss.str().c_str());
    std::cerr << oss.str();
    std::cerr.flush();
  }

  ::OleUninitialize();
  if (mutex) ::CloseHandle(mutex);
  return EXIT_SUCCESS;
}
