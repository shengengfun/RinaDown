#include "floating_ball_drop_target.h"

#include <shellapi.h>

#include "utils.h"

namespace {

// Payload cap: drops larger than this are discarded (plan A4 / R4).
constexpr size_t kMaxPayloadBytes = 4096;

}  // namespace

FloatingBallDropTarget::FloatingBallDropTarget(
    flutter::MethodChannel<flutter::EncodableValue>* channel)
    : channel_(channel) {}

HRESULT FloatingBallDropTarget::RegisterOn(HWND hwnd) {
  if (registered_hwnd_ == hwnd) return S_OK;
  Revoke();
  HRESULT hr = ::RegisterDragDrop(hwnd, this);
  if (SUCCEEDED(hr)) {
    registered_hwnd_ = hwnd;
    // UIPI hardening: when FluxDown runs elevated (High IL), Windows silently
    // filters drag-drop window messages posted by lower-integrity sources
    // (Explorer / browser at Medium IL), so OLE drops onto the ball never
    // fire. Explicitly allow them for this HWND. Best-effort; failures and
    // pre-Win7 systems are harmless (equal-IL drops already work).
    static constexpr UINT kWmCopyGlobalData = 0x0049;  // undocumented, no macro
    ::ChangeWindowMessageFilterEx(hwnd, WM_DROPFILES, MSGFLT_ALLOW, nullptr);
    ::ChangeWindowMessageFilterEx(hwnd, WM_COPYDATA, MSGFLT_ALLOW, nullptr);
    ::ChangeWindowMessageFilterEx(hwnd, kWmCopyGlobalData, MSGFLT_ALLOW,
                                  nullptr);
  }
  return hr;
}

void FloatingBallDropTarget::Revoke() {
  if (registered_hwnd_) {
    ::RevokeDragDrop(registered_hwnd_);
    registered_hwnd_ = nullptr;
  }
}

// ── IUnknown ────────────────────────────────────────────────────────────────

HRESULT FloatingBallDropTarget::QueryInterface(REFIID riid, void** ppv) {
  if (riid == IID_IUnknown || riid == IID_IDropTarget) {
    *ppv = static_cast<IDropTarget*>(this);
    AddRef();
    return S_OK;
  }
  *ppv = nullptr;
  return E_NOINTERFACE;
}

ULONG FloatingBallDropTarget::AddRef() {
  return ::InterlockedIncrement(&ref_count_);
}

ULONG FloatingBallDropTarget::Release() {
  LONG count = ::InterlockedDecrement(&ref_count_);
  if (count == 0) delete this;
  return count;
}

// ── IDropTarget ─────────────────────────────────────────────────────────────

HRESULT FloatingBallDropTarget::DragEnter(IDataObject* data_obj,
                                          DWORD /*key_state*/, POINTL /*pt*/,
                                          DWORD* effect) {
  if (HasSupportedFormat(data_obj)) {
    *effect = DROPEFFECT_COPY;
    if (channel_) {
      channel_->InvokeMethod("onDragEnter", nullptr);
    }
  } else {
    *effect = DROPEFFECT_NONE;
  }
  return S_OK;
}

HRESULT FloatingBallDropTarget::DragOver(DWORD /*key_state*/, POINTL /*pt*/,
                                         DWORD* effect) {
  *effect = DROPEFFECT_COPY;
  return S_OK;
}

HRESULT FloatingBallDropTarget::DragLeave() {
  if (channel_) {
    channel_->InvokeMethod("onDragLeave", nullptr);
  }
  return S_OK;
}

HRESULT FloatingBallDropTarget::Drop(IDataObject* data_obj,
                                     DWORD /*key_state*/, POINTL /*pt*/,
                                     DWORD* effect) {
  *effect = DROPEFFECT_COPY;
  std::string kind;
  std::vector<std::string> values;
  if (ExtractPayload(data_obj, &kind, &values) && channel_) {
    flutter::EncodableList list;
    for (const auto& v : values) {
      list.emplace_back(v);
    }
    auto args = std::make_unique<flutter::EncodableValue>(
        flutter::EncodableMap{
            {flutter::EncodableValue("kind"), flutter::EncodableValue(kind)},
            {flutter::EncodableValue("values"),
             flutter::EncodableValue(std::move(list))},
        });
    channel_->InvokeMethod("onDropPayload", std::move(args));
  } else if (channel_) {
    // No usable payload — still clear the hover highlight.
    channel_->InvokeMethod("onDragLeave", nullptr);
  }
  return S_OK;
}

// ── Payload extraction ──────────────────────────────────────────────────────

bool FloatingBallDropTarget::HasSupportedFormat(IDataObject* data_obj) {
  FORMATETC fmt_files = {CF_HDROP, nullptr, DVASPECT_CONTENT, -1,
                         TYMED_HGLOBAL};
  FORMATETC fmt_text = {CF_UNICODETEXT, nullptr, DVASPECT_CONTENT, -1,
                        TYMED_HGLOBAL};
  return data_obj->QueryGetData(&fmt_files) == S_OK ||
         data_obj->QueryGetData(&fmt_text) == S_OK;
}

bool FloatingBallDropTarget::ExtractPayload(IDataObject* data_obj,
                                            std::string* kind,
                                            std::vector<std::string>* values) {
  // 1. Files (CF_HDROP) take priority.
  FORMATETC fmt_files = {CF_HDROP, nullptr, DVASPECT_CONTENT, -1,
                         TYMED_HGLOBAL};
  STGMEDIUM medium = {};
  if (data_obj->GetData(&fmt_files, &medium) == S_OK) {
    HDROP hdrop = static_cast<HDROP>(::GlobalLock(medium.hGlobal));
    if (hdrop) {
      UINT count = ::DragQueryFileW(hdrop, 0xFFFFFFFF, nullptr, 0);
      for (UINT i = 0; i < count; ++i) {
        wchar_t path[MAX_PATH] = {};
        if (::DragQueryFileW(hdrop, i, path, MAX_PATH) > 0) {
          values->push_back(Utf8FromUtf16(path));
        }
      }
      ::GlobalUnlock(medium.hGlobal);
    }
    ::ReleaseStgMedium(&medium);
    if (!values->empty()) {
      *kind = "files";
      return true;
    }
  }

  // 2. Unicode text (browser URL drags arrive as CF_UNICODETEXT).
  FORMATETC fmt_text = {CF_UNICODETEXT, nullptr, DVASPECT_CONTENT, -1,
                        TYMED_HGLOBAL};
  if (data_obj->GetData(&fmt_text, &medium) == S_OK) {
    bool ok = false;
    const wchar_t* text =
        static_cast<const wchar_t*>(::GlobalLock(medium.hGlobal));
    if (text) {
      size_t byte_len = ::GlobalSize(medium.hGlobal);
      if (byte_len <= kMaxPayloadBytes) {
        std::string utf8 = Utf8FromUtf16(text);
        if (!utf8.empty()) {
          values->push_back(std::move(utf8));
          *kind = "text";
          ok = true;
        }
      }
      // >4KB → discard silently (coarse filter; plan R4)
      ::GlobalUnlock(medium.hGlobal);
    }
    ::ReleaseStgMedium(&medium);
    return ok;
  }

  return false;
}
