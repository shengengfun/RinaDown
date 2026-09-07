#ifndef RUNNER_FLOATING_BALL_DROP_TARGET_H_
#define RUNNER_FLOATING_BALL_DROP_TARGET_H_

#include <flutter/encodable_value.h>
#include <flutter/method_channel.h>
#include <ole2.h>
#include <windows.h>

#include <string>
#include <vector>

// IDropTarget for the floating ball layered window (plan S1.2).
//
// Registered on the Dart-created ball HWND via RegisterDragDrop on the
// platform thread (merged-thread model; process initialized with
// OleInitialize in main.cpp). Forwards payloads to Dart through the
// com.fluxdown/floating_ball MethodChannel:
//   - onDragEnter / onDragLeave  → drag-target visual variant switching
//   - onDropPayload {kind: "files"|"text", values: [...]}
//
// Native side does only coarse filtering (registered clipboard formats +
// 4KB payload cap); semantic URL validation stays in Dart (plan A4).
class FloatingBallDropTarget : public IDropTarget {
 public:
  explicit FloatingBallDropTarget(
      flutter::MethodChannel<flutter::EncodableValue>* channel);

  // Registers this target on |hwnd|. Returns the RegisterDragDrop HRESULT.
  HRESULT RegisterOn(HWND hwnd);
  // Revokes registration (idempotent).
  void Revoke();

  // IUnknown
  HRESULT STDMETHODCALLTYPE QueryInterface(REFIID riid, void** ppv) override;
  ULONG STDMETHODCALLTYPE AddRef() override;
  ULONG STDMETHODCALLTYPE Release() override;

  // IDropTarget
  HRESULT STDMETHODCALLTYPE DragEnter(IDataObject* data_obj, DWORD key_state,
                                      POINTL pt, DWORD* effect) override;
  HRESULT STDMETHODCALLTYPE DragOver(DWORD key_state, POINTL pt,
                                     DWORD* effect) override;
  HRESULT STDMETHODCALLTYPE DragLeave() override;
  HRESULT STDMETHODCALLTYPE Drop(IDataObject* data_obj, DWORD key_state,
                                 POINTL pt, DWORD* effect) override;

 private:
  // Extracts CF_HDROP file paths or CF_UNICODETEXT (≤4KB) from |data_obj|.
  // Returns true and fills |kind|/|values| when a supported format is found.
  static bool ExtractPayload(IDataObject* data_obj, std::string* kind,
                             std::vector<std::string>* values);
  static bool HasSupportedFormat(IDataObject* data_obj);

  flutter::MethodChannel<flutter::EncodableValue>* channel_;  // not owned
  HWND registered_hwnd_ = nullptr;
  LONG ref_count_ = 1;
};

#endif  // RUNNER_FLOATING_BALL_DROP_TARGET_H_
