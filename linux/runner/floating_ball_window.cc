#include "floating_ball_window.h"

#include <gtk/gtk.h>
#ifdef GDK_WINDOWING_X11
#include <gdk/gdkx.h>
#endif

#include <cairo.h>
#include <math.h>
#include <string.h>

// =============================================================================
// Spec constants — MUST track lib/src/services/floating_ball/floating_ball_renderer.dart
// (kBallWindowSize / kBallHitRadius) and the A7/S3.4 dock-position formula.
// =============================================================================

static const int kDefaultLogicalSize = 72;   // logical px, square window
static const double kHitRadius = 28.0;       // logical px, circle at (size/2, size/2)
static const int kDockEdgePad = 8;           // logical px from work-area edge
static const double kDockVerticalFrac = 0.4;
static const guint kMoveDebounceMs = 300;
static const gint kMaxDropPayloadBytes = 4096;  // matches windows/runner (plan R4)

// =============================================================================
// Edge-dock auto-collapse (Thunder-style) — spec parity with
// lib/src/services/floating_ball/win32_ball_window.dart's "贴边收起" section.
// =============================================================================

// Snap threshold (logical px): ball edge within this distance of the
// work-area left/right/top edge on drag release -> dock to that edge.
static const int kDockSnapThreshold = 12;
// Reveal width (logical px) left on-screen once collapsed.
static const int kDockRevealWidth = 14;
// Cursor-left-the-ball delay before collapsing (ms).
static const guint kDockCollapseDelayMs = 800;
// Dock/collapse/expand slide animation duration (ms) and tick interval (ms).
static const guint kDockAnimMs = 160;
static const guint kDockAnimIntervalMs = 16;

// Which work-area edge (if any) the ball is currently pinned to.
enum DockEdge {
  kDockEdgeNone = 0,
  kDockEdgeLeft,
  kDockEdgeRight,
  kDockEdgeTop,
};

// =============================================================================
// Controller state
// =============================================================================

struct _FloatingBallWindow {
  FlMethodChannel* channel;  // owned

  gboolean is_x11;

  GtkWidget* window;  // owned by GTK; NULL when not created or destroyed

  // Bitmap cache (straight-alpha RGBA pushed from Dart).
  guint8* bitmap;        // g_malloc'd; NULL if nothing pushed yet
  int bitmap_width;      // physical px
  int bitmap_height;     // physical px
  double bitmap_scale;   // devicePixelRatio the bitmap was rendered at

  int logical_size;  // current logical window side length (square)

  // Click vs. drag disambiguation (button-press/motion/release).
  gboolean pointer_down;
  gboolean dragging;
  double press_root_x;
  double press_root_y;

  // configure-event -> onBallMoved debounce.
  guint move_debounce_id;  // 0 = none pending

  // Suppresses configure-event handling across hide/destroy/show
  // transitions so window-manager teardown noise never reaches Dart as a
  // garbage coordinate (S0.5).
  gboolean destroying;

  // Drag-and-drop hover dedup (only send onDragEnter once per hover run).
  gboolean drag_hover;

  // Edge-dock auto-collapse (Thunder-style) state machine.
  DockEdge dock_edge;       // kDockEdgeNone when free-floating
  gboolean collapsed;       // TRUE = only the reveal sliver is on-screen
  guint collapse_delay_id;  // pending "cursor left -> collapse" timeout; 0 = none

  // Dock/collapse/expand slide animation (position tween).
  guint anim_id;         // g_timeout_add id driving the current animation; 0 = none
  gint64 anim_start_us;  // g_get_monotonic_time() at animation start
  gint anim_from_x, anim_from_y;
  gint anim_to_x, anim_to_y;

  // Incremented before every animation-driven gtk_window_move() and
  // decremented by the next configure-event: while > 0, that settle is our
  // own doing, not a user drag, so it must not re-arm the onBallMoved
  // debounce (only real drag-release coordinates get persisted — collapse/
  // expand/dock-snap displacement must never echo back to Dart).
  guint suppress_configure_count;
};

// Forward declarations: dock/collapse handlers referenced by callbacks wired
// up (button/motion/drag-motion events, ensure_window's g_signal_connect
// calls) before their own definitions in the edge-dock section below.
static void apply_input_shape(FloatingBallWindow* self);
static void evaluate_dock_snap(FloatingBallWindow* self);
static void cancel_dock_timeouts(FloatingBallWindow* self);
static void dock_expand_if_collapsed(FloatingBallWindow* self);
static gboolean enter_notify_event_cb(GtkWidget*, GdkEventCrossing*, gpointer);
static gboolean leave_notify_event_cb(GtkWidget*, GdkEventCrossing*, gpointer);

// =============================================================================
// Capability detection (X11 vs Wayland)
// =============================================================================

static gboolean detect_is_x11() {
#ifdef GDK_WINDOWING_X11
  GdkDisplay* display = gdk_display_get_default();
  if (display != nullptr && GDK_IS_X11_DISPLAY(display)) {
    return TRUE;
  }
#endif
  return FALSE;
}

static void send_capability(FloatingBallWindow* self) {
  g_autoptr(FlValue) args = fl_value_new_map();
  fl_value_set_string_take(
      args, "mode", fl_value_new_string(self->is_x11 ? "x11" : "wayland"));
  fl_method_channel_invoke_method(self->channel, "onCapability", args,
                                  nullptr, nullptr, nullptr);
}

// =============================================================================
// onBallMoved debounce (configure-event fires repeatedly while the window
// manager drags/places the window; only report the settled position).
// =============================================================================

static void cancel_move_debounce(FloatingBallWindow* self) {
  if (self->move_debounce_id != 0) {
    g_source_remove(self->move_debounce_id);
    self->move_debounce_id = 0;
  }
}

static gboolean move_debounce_cb(gpointer user_data) {
  FloatingBallWindow* self = (FloatingBallWindow*)user_data;
  self->move_debounce_id = 0;
  if (self->window == nullptr || self->destroying) {
    return G_SOURCE_REMOVE;
  }
  gint x = 0, y = 0;
  gtk_window_get_position(GTK_WINDOW(self->window), &x, &y);
  g_autoptr(FlValue) args = fl_value_new_map();
  fl_value_set_string_take(args, "x", fl_value_new_float(x));
  fl_value_set_string_take(args, "y", fl_value_new_float(y));
  fl_method_channel_invoke_method(self->channel, "onBallMoved", args,
                                  nullptr, nullptr, nullptr);
  // Drag has settled — X11 hands the whole gesture to the WM, so debounce
  // quiescence is the only "drag ended" signal available; re-evaluate
  // edge-dock snap now that onBallMoved above has already captured the raw
  // release coordinates (item 2).
  evaluate_dock_snap(self);
  return G_SOURCE_REMOVE;
}

static gboolean configure_event_cb(GtkWidget* /*widget*/,
                                   GdkEventConfigure* /*event*/,
                                   gpointer user_data) {
  FloatingBallWindow* self = (FloatingBallWindow*)user_data;
  if (self->destroying) {
    // Hide/destroy transition in progress — WM teardown can emit bogus
    // ConfigureNotify (e.g. (-32000,-32000)-style values on some WMs);
    // never let those reach Dart.
    return FALSE;
  }
  if (self->suppress_configure_count > 0) {
    // This settle was caused by our own dock/collapse/expand animation
    // (gtk_window_move from anim_tick_cb), not a user drag — swallow it so
    // onBallMoved never reports animation-driven displacement.
    self->suppress_configure_count--;
    return FALSE;
  }
  cancel_move_debounce(self);
  self->move_debounce_id = g_timeout_add(kMoveDebounceMs, move_debounce_cb, self);
  return FALSE;
}

// =============================================================================
// Click vs. drag: button-press records the anchor, motion past the
// GtkSettings drag threshold hands off to the window manager via
// gtk_window_begin_move_drag (which owns the rest of the gesture per
// _NET_WM_MOVERESIZE — no further release event is guaranteed), an
// unmoved release is a click.
// =============================================================================

static gboolean button_press_event_cb(GtkWidget* /*widget*/,
                                      GdkEventButton* event,
                                      gpointer user_data) {
  FloatingBallWindow* self = (FloatingBallWindow*)user_data;
  if (event->button == 3) {
    if (self->collapsed) {
      // Collapsed: only the reveal sliver is visible — no menu until the
      // user hovers it open first (item 8; matches Win32BallWindow's
      // `hovering && !dragging && !collapsed` context-menu gate).
      return FALSE;
    }
    // Right-click: ask Dart to assemble the (i18n'd) context menu instead of
    // entering the left-button click/drag state machine below.
    fl_method_channel_invoke_method(self->channel, "onContextMenuRequested",
                                    nullptr, nullptr, nullptr, nullptr);
    return FALSE;
  }
  if (event->button != 1) {
    return FALSE;
  }
  self->pointer_down = TRUE;
  self->dragging = FALSE;
  self->press_root_x = event->x_root;
  self->press_root_y = event->y_root;
  return FALSE;
}

static gboolean motion_notify_event_cb(GtkWidget* widget, GdkEventMotion* event,
                                       gpointer user_data) {
  FloatingBallWindow* self = (FloatingBallWindow*)user_data;
  if (!self->pointer_down || self->dragging) {
    return FALSE;
  }

  gint threshold = 8;
  GtkSettings* settings = gtk_widget_get_settings(widget);
  if (settings != nullptr) {
    g_object_get(settings, "gtk-dnd-drag-threshold", &threshold, nullptr);
  }

  double dx = event->x_root - self->press_root_x;
  double dy = event->y_root - self->press_root_y;
  if (hypot(dx, dy) < threshold) {
    return FALSE;
  }

  self->dragging = TRUE;
  // Drag away from a dock unconditionally clears dock/collapse state (item
  // 6) — a ball being actively dragged is never "docked."
  self->dock_edge = kDockEdgeNone;
  if (self->collapsed) {
    self->collapsed = FALSE;
    apply_input_shape(self);
  }
  cancel_dock_timeouts(self);
  gtk_window_begin_move_drag(GTK_WINDOW(widget), 1, (gint)event->x_root,
                             (gint)event->y_root, event->time);
  return FALSE;
}

static gboolean button_release_event_cb(GtkWidget* /*widget*/,
                                        GdkEventButton* event,
                                        gpointer user_data) {
  FloatingBallWindow* self = (FloatingBallWindow*)user_data;
  if (event->button != 1) {
    return FALSE;
  }
  gboolean was_dragging = self->dragging;
  self->pointer_down = FALSE;
  self->dragging = FALSE;
  if (!was_dragging) {
    fl_method_channel_invoke_method(self->channel, "onBallClicked", nullptr,
                                    nullptr, nullptr, nullptr);
  }
  return FALSE;
}

// =============================================================================
// Circular input shape (hit-test region) — click-through outside the circle.
// =============================================================================

static cairo_region_t* create_circle_region(double cx, double cy, double radius) {
  cairo_region_t* region = cairo_region_create();
  int top = (int)floor(cy - radius);
  int bottom = (int)ceil(cy + radius);
  double r2 = radius * radius;
  for (int y = top; y < bottom; y++) {
    double dy = (y + 0.5) - cy;
    double dy2 = dy * dy;
    if (dy2 > r2) {
      continue;
    }
    double half_width = sqrt(r2 - dy2);
    int left = (int)floor(cx - half_width);
    int right = (int)ceil(cx + half_width);
    if (right <= left) {
      continue;
    }
    cairo_rectangle_int_t rect = {left, y, right - left, 1};
    cairo_region_union_rectangle(region, &rect);
  }
  return region;
}

static void apply_input_shape(FloatingBallWindow* self) {
  if (self->window == nullptr || !gtk_widget_get_realized(self->window)) {
    return;
  }
  if (self->collapsed) {
    // Collapsed: most of the window sits off-screen and only a
    // kDockRevealWidth-wide sliver is visible; a circular shape centered on
    // the (now mostly hidden) full window would miss most of that sliver, so
    // the whole window rectangle becomes the hit-test area instead (NULL
    // removes any existing custom input shape — item 5).
    gtk_widget_input_shape_combine_region(self->window, nullptr);
    return;
  }
  double size = self->logical_size;
  // Hit radius scales proportionally if the logical size ever deviates from
  // the 72px default (kept generic rather than hardcoding 36/28).
  double ratio = size / kDefaultLogicalSize;
  cairo_region_t* region =
      create_circle_region(size / 2.0, size / 2.0, kHitRadius * ratio);
  gtk_widget_input_shape_combine_region(self->window, region);
  cairo_region_destroy(region);
}

static void realize_cb(GtkWidget* /*widget*/, gpointer user_data) {
  apply_input_shape((FloatingBallWindow*)user_data);
}

// =============================================================================
// Drawing: straight-alpha RGBA (Dart) -> premultiplied ARGB32 (cairo, native-
// endian == little-endian BGRA in memory) blit.
// =============================================================================

static gboolean draw_cb(GtkWidget* /*widget*/, cairo_t* cr, gpointer user_data) {
  FloatingBallWindow* self = (FloatingBallWindow*)user_data;

  // app-paintable suppresses GTK's own background clear; the RGBA visual
  // needs an explicit clear each frame or stale pixels can show at the
  // circle's edge as the window manager recomposites.
  cairo_save(cr);
  cairo_set_operator(cr, CAIRO_OPERATOR_CLEAR);
  cairo_paint(cr);
  cairo_restore(cr);

  if (self->bitmap == nullptr || self->bitmap_width <= 0 ||
      self->bitmap_height <= 0) {
    return FALSE;
  }

  const int w = self->bitmap_width;
  const int h = self->bitmap_height;
  const int stride = cairo_format_stride_for_width(CAIRO_FORMAT_ARGB32, w);
  guint8* argb = (guint8*)g_malloc0((gsize)stride * h);

  for (int y = 0; y < h; y++) {
    const guint8* src_row = self->bitmap + (size_t)y * w * 4;
    guint32* dst_row = (guint32*)(argb + (size_t)y * stride);
    for (int x = 0; x < w; x++) {
      const guint8 r = src_row[x * 4 + 0];
      const guint8 g = src_row[x * 4 + 1];
      const guint8 b = src_row[x * 4 + 2];
      const guint8 a = src_row[x * 4 + 3];
      // Premultiply straight alpha (cairo ARGB32 requires premultiplied
      // pixels); native-endian 0xAARRGGBB word, i.e. B,G,R,A bytes on LE.
      const guint8 pr = (guint8)((r * a + 127) / 255);
      const guint8 pg = (guint8)((g * a + 127) / 255);
      const guint8 pb = (guint8)((b * a + 127) / 255);
      dst_row[x] = ((guint32)a << 24) | ((guint32)pr << 16) |
                  ((guint32)pg << 8) | (guint32)pb;
    }
  }

  cairo_surface_t* surface =
      cairo_image_surface_create_for_data(argb, CAIRO_FORMAT_ARGB32, w, h, stride);
  if (cairo_surface_status(surface) == CAIRO_STATUS_SUCCESS) {
    // The bitmap's own scale (Dart's devicePixelRatio) is the source of
    // truth for how many device pixels map to one logical/user-space unit —
    // independent of GTK's own integer widget scale factor.
    cairo_surface_set_device_scale(surface, self->bitmap_scale, self->bitmap_scale);
    cairo_set_source_surface(cr, surface, 0, 0);
    cairo_paint(cr);
  }
  cairo_surface_destroy(surface);
  g_free(argb);

  return FALSE;
}

static void on_window_destroy(GtkWidget* /*widget*/, gpointer user_data) {
  FloatingBallWindow* self = (FloatingBallWindow*)user_data;
  self->window = nullptr;
}

// =============================================================================
// Drag-and-drop (external drops onto the ball).
// =============================================================================

static const GtkTargetEntry kDropTargets[] = {
    {(gchar*)"text/uri-list", 0, 0},
    {(gchar*)"text/plain", 0, 0},
    {(gchar*)"UTF8_STRING", 0, 0},
};

static gboolean drag_motion_cb(GtkWidget* /*widget*/, GdkDragContext* /*context*/,
                               gint /*x*/, gint /*y*/, guint /*time*/,
                               gpointer user_data) {
  FloatingBallWindow* self = (FloatingBallWindow*)user_data;
  // External drag hovering the collapsed reveal sliver -> expand
  // immediately so the drop actually lands on the ball (item 7). Idempotent
  // no-op once already expanded/mid-expand.
  dock_expand_if_collapsed(self);
  if (!self->drag_hover) {
    self->drag_hover = TRUE;
    fl_method_channel_invoke_method(self->channel, "onDragEnter", nullptr,
                                    nullptr, nullptr, nullptr);
  }
  // GTK_DEST_DEFAULT_MOTION already calls gdk_drag_status() for us and
  // ignores this return value.
  return TRUE;
}

static void drag_leave_cb(GtkWidget* /*widget*/, GdkDragContext* /*context*/,
                          guint /*time*/, gpointer user_data) {
  FloatingBallWindow* self = (FloatingBallWindow*)user_data;
  if (self->drag_hover) {
    self->drag_hover = FALSE;
    fl_method_channel_invoke_method(self->channel, "onDragLeave", nullptr,
                                    nullptr, nullptr, nullptr);
  }
}

static void drag_data_received_cb(GtkWidget* /*widget*/,
                                  GdkDragContext* /*context*/, gint /*x*/,
                                  gint /*y*/, GtkSelectionData* data,
                                  guint /*info*/, guint /*time*/,
                                  gpointer user_data) {
  FloatingBallWindow* self = (FloatingBallWindow*)user_data;
  self->drag_hover = FALSE;

  const gint len = gtk_selection_data_get_length(data);
  if (len <= 0 || len > kMaxDropPayloadBytes) {
    // Invalid or >4KB payload — discard silently (plan R4 coarse filter).
    // GTK_DEST_DEFAULT_DROP still calls gtk_drag_finish() for us.
    fl_method_channel_invoke_method(self->channel, "onDragLeave", nullptr,
                                    nullptr, nullptr, nullptr);
    return;
  }

  const gchar* kind = nullptr;
  FlValue* list = nullptr;

  gchar** uris = gtk_selection_data_get_uris(data);
  if (uris != nullptr) {
    // Files take priority (matches windows/runner/floating_ball_drop_target.cpp):
    // a drop mixing file:// URIs with other URIs is reported as files-only.
    FlValue* file_list = fl_value_new_list();
    FlValue* text_list = fl_value_new_list();
    for (gchar** p = uris; *p != nullptr; p++) {
      if (g_str_has_prefix(*p, "file://")) {
        gchar* path = g_filename_from_uri(*p, nullptr, nullptr);
        if (path != nullptr) {
          fl_value_append_take(file_list, fl_value_new_string(path));
          g_free(path);
        }
      } else {
        fl_value_append_take(text_list, fl_value_new_string(*p));
      }
    }
    g_strfreev(uris);
    if (fl_value_get_length(file_list) > 0) {
      kind = "files";
      list = file_list;
      fl_value_unref(text_list);
    } else if (fl_value_get_length(text_list) > 0) {
      kind = "text";
      list = text_list;
      fl_value_unref(file_list);
    } else {
      fl_value_unref(file_list);
      fl_value_unref(text_list);
    }
  } else {
    guchar* text = gtk_selection_data_get_text(data);
    if (text != nullptr) {
      list = fl_value_new_list();
      fl_value_append_take(list, fl_value_new_string((const gchar*)text));
      kind = "text";
      g_free(text);
    }
  }

  if (list == nullptr) {
    fl_method_channel_invoke_method(self->channel, "onDragLeave", nullptr,
                                    nullptr, nullptr, nullptr);
    return;
  }

  g_autoptr(FlValue) map = fl_value_new_map();
  fl_value_set_string_take(map, "kind", fl_value_new_string(kind));
  fl_value_set_string_take(map, "values", list);
  fl_method_channel_invoke_method(self->channel, "onDropPayload", map,
                                  nullptr, nullptr, nullptr);
}

// =============================================================================
// Context menu (native right-click -> onContextMenuRequested -> Dart
// assembles an i18n menu -> showContextMenu -> native popup ->
// onMenuAction). Dart never sees GTK objects, only the ids it chose.
// =============================================================================

static void context_menu_item_activate_cb(GtkMenuItem* item, gpointer user_data) {
  FloatingBallWindow* self = (FloatingBallWindow*)user_data;
  const int id = GPOINTER_TO_INT(
      g_object_get_data(G_OBJECT(item), "fluxdown-menu-item-id"));
  g_autoptr(FlValue) args = fl_value_new_map();
  fl_value_set_string_take(args, "id", fl_value_new_int(id));
  fl_method_channel_invoke_method(self->channel, "onMenuAction", args,
                                  nullptr, nullptr, nullptr);
}

// A selection sends onMenuAction *before* this runs: on button-release
// gtk_menu_shell_activate_item(menu_item, force_deactivate=TRUE) takes its
// own refs on the shell/item, emits ::deactivate (-> here, dropping our
// ref_sink) *then* activates the item (-> onMenuAction), only releasing its
// refs afterwards — so our unref never finalizes the menu before the item's
// "activate" handler has run. A cancel (click-away/Escape) instead calls
// gtk_menu_shell_deactivate() directly with no activation, so no
// onMenuAction is sent, matching the contract. Either way this drops the
// ref taken in handle_show_context_menu(), finalizing the unparented popup.
static void context_menu_deactivate_cb(GtkMenuShell* menu_shell,
                                       gpointer /*user_data*/) {
  g_object_unref(menu_shell);
}

// =============================================================================
// Window construction (lazy — first showBall call).
// =============================================================================

static void ensure_window(FloatingBallWindow* self) {
  if (self->window != nullptr) {
    return;
  }

  // Plain top-level window — NEVER gtk_application_window_new() /
  // gtk_application_add_window(). A GApplication quits when the last window
  // it tracks closes; the ball must not be tracked, since it can legitimately
  // outlive the (hidden) main window.
  GtkWidget* window = gtk_window_new(GTK_WINDOW_TOPLEVEL);
  self->window = window;

  gtk_window_set_decorated(GTK_WINDOW(window), FALSE);
  gtk_window_set_keep_above(GTK_WINDOW(window), TRUE);
  gtk_window_set_skip_taskbar_hint(GTK_WINDOW(window), TRUE);
  gtk_window_set_skip_pager_hint(GTK_WINDOW(window), TRUE);
  gtk_window_stick(GTK_WINDOW(window));
  gtk_window_set_resizable(GTK_WINDOW(window), FALSE);
  // A dumb overlay must never steal keyboard focus from the user's active
  // window (mirrors the Windows WS_EX_NOACTIVATE-style behavior implied by
  // the "dumb window" contract).
  gtk_window_set_accept_focus(GTK_WINDOW(window), FALSE);
  gtk_window_set_title(GTK_WINDOW(window), "FluxDown Floating Ball");
  gtk_widget_set_size_request(window, self->logical_size, self->logical_size);

  GdkScreen* screen = gtk_window_get_screen(GTK_WINDOW(window));
  GdkVisual* rgba_visual =
      screen != nullptr ? gdk_screen_get_rgba_visual(screen) : nullptr;
  if (rgba_visual != nullptr) {
    gtk_widget_set_visual(window, rgba_visual);
  }
  // No RGBA visual available (e.g. no compositor) — falls back to the
  // default opaque visual; the ball just won't be see-through.
  gtk_widget_set_app_paintable(window, TRUE);

  gtk_widget_add_events(window, GDK_BUTTON_PRESS_MASK | GDK_BUTTON_RELEASE_MASK |
                                    GDK_POINTER_MOTION_MASK | GDK_STRUCTURE_MASK |
                                    GDK_ENTER_NOTIFY_MASK | GDK_LEAVE_NOTIFY_MASK);

  g_signal_connect(window, "realize", G_CALLBACK(realize_cb), self);
  g_signal_connect(window, "draw", G_CALLBACK(draw_cb), self);
  g_signal_connect(window, "button-press-event", G_CALLBACK(button_press_event_cb), self);
  g_signal_connect(window, "motion-notify-event", G_CALLBACK(motion_notify_event_cb), self);
  g_signal_connect(window, "button-release-event", G_CALLBACK(button_release_event_cb), self);
  g_signal_connect(window, "configure-event", G_CALLBACK(configure_event_cb), self);
  g_signal_connect(window, "destroy", G_CALLBACK(on_window_destroy), self);
  g_signal_connect(window, "enter-notify-event", G_CALLBACK(enter_notify_event_cb), self);
  g_signal_connect(window, "leave-notify-event", G_CALLBACK(leave_notify_event_cb), self);

  gtk_drag_dest_set(window, GTK_DEST_DEFAULT_ALL, kDropTargets,
                    G_N_ELEMENTS(kDropTargets), GDK_ACTION_COPY);
  g_signal_connect(window, "drag-motion", G_CALLBACK(drag_motion_cb), self);
  g_signal_connect(window, "drag-leave", G_CALLBACK(drag_leave_cb), self);
  g_signal_connect(window, "drag-data-received", G_CALLBACK(drag_data_received_cb), self);
}

// =============================================================================
// Positioning (A7 dock rule + fall-on-screen clamp).
// =============================================================================

static void get_primary_workarea(GdkRectangle* out) {
  GdkDisplay* display = gdk_display_get_default();
  GdkMonitor* monitor =
      display != nullptr ? gdk_display_get_primary_monitor(display) : nullptr;
  if (monitor == nullptr && display != nullptr &&
      gdk_display_get_n_monitors(display) > 0) {
    monitor = gdk_display_get_monitor(display, 0);
  }
  if (monitor != nullptr) {
    gdk_monitor_get_workarea(monitor, out);
    return;
  }
  // No monitor info (headless/misconfigured) — sane fallback so we never
  // gtk_window_move() to garbage coordinates.
  out->x = 0;
  out->y = 0;
  out->width = 1280;
  out->height = 720;
}

// Default dock: primary work-area right edge, 40% down, 8px edge pad (A7) —
// mirrors Win32BallWindow._defaultDock. Otherwise clamps the requested
// coordinates to the work area (fall-on-screen snap).
static void resolve_position(FloatingBallWindow* self, double requested_x,
                             double requested_y, gint* out_x, gint* out_y) {
  GdkRectangle wa;
  get_primary_workarea(&wa);
  const int size = self->logical_size;

  if (requested_x < 0 || requested_y < 0) {
    *out_x = wa.x + wa.width - size - kDockEdgePad;
    *out_y = wa.y + (gint)lround(wa.height * kDockVerticalFrac);
    return;
  }

  gint x = (gint)lround(requested_x);
  gint y = (gint)lround(requested_y);
  x = MAX(wa.x, MIN(x, wa.x + wa.width - size));
  y = MAX(wa.y, MIN(y, wa.y + wa.height - size));
  *out_x = x;
  *out_y = y;
}

// =============================================================================
// Edge-dock auto-collapse (Thunder-style): snap-to-edge on drag release,
// slide-to-reveal-sliver after the cursor leaves, slide-back on hover/drop.
// Spec + formulas mirror lib/src/services/floating_ball/win32_ball_window.dart
// 1:1 (dock threshold, reveal width, collapse delay, ease-out cubic timing).
// =============================================================================

// Work area of whichever monitor the ball is actually on (falls back to the
// primary monitor pre-realize/headless, same as get_primary_workarea).
static void get_ball_workarea(FloatingBallWindow* self, GdkRectangle* out) {
  if (self->window != nullptr) {
    GdkWindow* gdk_window = gtk_widget_get_window(self->window);
    GdkDisplay* display = gdk_display_get_default();
    if (gdk_window != nullptr && display != nullptr) {
      GdkMonitor* monitor = gdk_display_get_monitor_at_window(display, gdk_window);
      if (monitor != nullptr) {
        gdk_monitor_get_workarea(monitor, out);
        return;
      }
    }
  }
  get_primary_workarea(out);
}

// Fully-visible docked position: ball flush against |self->dock_edge|.
static void docked_expanded_pos(FloatingBallWindow* self, const GdkRectangle* wa,
                                gint* out_x, gint* out_y) {
  gint x = 0, y = 0;
  gtk_window_get_position(GTK_WINDOW(self->window), &x, &y);
  const int size = self->logical_size;
  switch (self->dock_edge) {
    case kDockEdgeLeft:
      *out_x = wa->x;
      *out_y = y;
      return;
    case kDockEdgeRight:
      *out_x = wa->x + wa->width - size;
      *out_y = y;
      return;
    case kDockEdgeTop:
      *out_x = x;
      *out_y = wa->y;
      return;
    case kDockEdgeNone:
    default:
      *out_x = x;
      *out_y = y;
      return;
  }
}

// Collapsed position: only kDockRevealWidth logical px left on-screen.
static void docked_collapsed_pos(FloatingBallWindow* self, const GdkRectangle* wa,
                                 gint* out_x, gint* out_y) {
  gint x = 0, y = 0;
  gtk_window_get_position(GTK_WINDOW(self->window), &x, &y);
  const int size = self->logical_size;
  switch (self->dock_edge) {
    case kDockEdgeLeft:
      *out_x = wa->x - size + kDockRevealWidth;
      *out_y = y;
      return;
    case kDockEdgeRight:
      *out_x = wa->x + wa->width - kDockRevealWidth;
      *out_y = y;
      return;
    case kDockEdgeTop:
      *out_x = x;
      *out_y = wa->y - size + kDockRevealWidth;
      return;
    case kDockEdgeNone:
    default:
      *out_x = x;
      *out_y = y;
      return;
  }
}

// Cancels only the pending "cursor left -> collapse" timer.
static void cancel_collapse_delay(FloatingBallWindow* self) {
  if (self->collapse_delay_id != 0) {
    g_source_remove(self->collapse_delay_id);
    self->collapse_delay_id = 0;
  }
}

// Cancels the collapse-delay timer AND any in-flight slide animation.
static void cancel_dock_timeouts(FloatingBallWindow* self) {
  cancel_collapse_delay(self);
  if (self->anim_id != 0) {
    g_source_remove(self->anim_id);
    self->anim_id = 0;
  }
}

// 16ms tween tick — ease-out cubic position interpolation, matching
// Win32BallWindow._stepAnim's formula and kDockAnimMs duration exactly.
static gboolean anim_tick_cb(gpointer user_data) {
  FloatingBallWindow* self = (FloatingBallWindow*)user_data;
  if (self->window == nullptr || self->destroying) {
    self->anim_id = 0;
    return G_SOURCE_REMOVE;
  }

  const gint64 elapsed_us = g_get_monotonic_time() - self->anim_start_us;
  double t = (elapsed_us / 1000.0) / (double)kDockAnimMs;
  gboolean done = FALSE;
  if (t >= 1.0) {
    t = 1.0;
    done = TRUE;
  }
  const double e = 1.0 - pow(1.0 - t, 3.0);  // ease-out cubic
  const gint x =
      (gint)lround(self->anim_from_x + (self->anim_to_x - self->anim_from_x) * e);
  const gint y =
      (gint)lround(self->anim_from_y + (self->anim_to_y - self->anim_from_y) * e);

  // This move's eventual configure-event must not re-arm the onBallMoved
  // debounce — only real drag-release coordinates get persisted, never
  // dock-snap/collapse/expand displacement (item 2/item 9).
  self->suppress_configure_count++;
  gtk_window_move(GTK_WINDOW(self->window), x, y);

  if (!done) {
    return G_SOURCE_CONTINUE;
  }
  self->anim_id = 0;
  // Settled: (re)apply the input shape matching the now-current collapsed
  // state. This is the one moment it's always safe to narrow to the circle
  // (ball fully at rest, nothing left off-screen mid-slide) as well as to
  // widen to the full-window rect (item 5).
  apply_input_shape(self);
  return G_SOURCE_REMOVE;
}

// Starts (or redirects an in-flight) slide to |target_x,target_y| — 160ms
// ease-out cubic (item 3).
static void start_dock_anim(FloatingBallWindow* self, gint target_x, gint target_y) {
  if (self->window == nullptr) {
    return;
  }
  gint cur_x = 0, cur_y = 0;
  gtk_window_get_position(GTK_WINDOW(self->window), &cur_x, &cur_y);
  if (self->anim_id == 0 && cur_x == target_x && cur_y == target_y) {
    // Already there and no animation running — just settle the shape.
    apply_input_shape(self);
    return;
  }
  if (self->anim_id != 0) {
    g_source_remove(self->anim_id);
  }
  self->anim_from_x = cur_x;
  self->anim_from_y = cur_y;
  self->anim_to_x = target_x;
  self->anim_to_y = target_y;
  self->anim_start_us = g_get_monotonic_time();
  self->anim_id = g_timeout_add(kDockAnimIntervalMs, anim_tick_cb, self);
}

// Drag-release/external-correction/show-time snap check (item 2, item 9):
// ball edge within kDockSnapThreshold logical px of the work-area left/
// right/top edge -> dock (pin fully visible flush against that edge).
// Edge priority (left > right > top) and formulas mirror
// Win32BallWindow._evaluateDock exactly.
static void evaluate_dock_snap(FloatingBallWindow* self) {
  if (self->window == nullptr) {
    return;
  }
  cancel_dock_timeouts(self);

  GdkRectangle wa;
  get_ball_workarea(self, &wa);
  gint x = 0, y = 0;
  gtk_window_get_position(GTK_WINDOW(self->window), &x, &y);
  const int size = self->logical_size;

  DockEdge edge = kDockEdgeNone;
  if (x - wa.x <= kDockSnapThreshold) {
    edge = kDockEdgeLeft;
  } else if ((wa.x + wa.width) - (x + size) <= kDockSnapThreshold) {
    edge = kDockEdgeRight;
  } else if (y - wa.y <= kDockSnapThreshold) {
    edge = kDockEdgeTop;
  }

  self->dock_edge = edge;
  self->collapsed = FALSE;
  if (edge == kDockEdgeNone) {
    apply_input_shape(self);
    return;
  }
  gint target_x = 0, target_y = 0;
  docked_expanded_pos(self, &wa, &target_x, &target_y);
  start_dock_anim(self, target_x, target_y);
}

// Fires kDockCollapseDelayMs after the cursor left an expanded docked ball
// with no re-entry (item 4).
static gboolean collapse_delay_cb(gpointer user_data) {
  FloatingBallWindow* self = (FloatingBallWindow*)user_data;
  self->collapse_delay_id = 0;
  if (self->window == nullptr || self->destroying ||
      self->dock_edge == kDockEdgeNone || self->collapsed) {
    return G_SOURCE_REMOVE;
  }
  self->collapsed = TRUE;
  // Safe to widen to the full-window rect immediately (item 5) — narrowing
  // back to the circle happens once the slide settles (anim_tick_cb).
  apply_input_shape(self);
  GdkRectangle wa;
  get_ball_workarea(self, &wa);
  gint target_x = 0, target_y = 0;
  docked_collapsed_pos(self, &wa, &target_x, &target_y);
  start_dock_anim(self, target_x, target_y);
  return G_SOURCE_REMOVE;
}

// Forces expand — called on hover-enter of the reveal sliver and on
// external drag-motion over it (item 4, item 7).
static void dock_expand_if_collapsed(FloatingBallWindow* self) {
  if (self->window == nullptr || self->dock_edge == kDockEdgeNone ||
      !self->collapsed) {
    return;
  }
  self->collapsed = FALSE;
  GdkRectangle wa;
  get_ball_workarea(self, &wa);
  gint target_x = 0, target_y = 0;
  docked_expanded_pos(self, &wa, &target_x, &target_y);
  start_dock_anim(self, target_x, target_y);
}

// GDK_CROSSING_NORMAL filters out synthetic crossing events GTK/GDK
// generates for grabs (context menu popup, button-press implicit grab,
// window-manager drag) — only real pointer entry drives the dock hover
// state machine (item 4).
static gboolean enter_notify_event_cb(GtkWidget* /*widget*/, GdkEventCrossing* event,
                                      gpointer user_data) {
  FloatingBallWindow* self = (FloatingBallWindow*)user_data;
  if (self->destroying || event->mode != GDK_CROSSING_NORMAL ||
      self->dock_edge == kDockEdgeNone) {
    return FALSE;
  }
  if (self->collapsed) {
    dock_expand_if_collapsed(self);
  } else {
    // Still (or again) expanded before the collapse delay elapsed — cancel
    // the pending collapse.
    cancel_collapse_delay(self);
  }
  return FALSE;
}

static gboolean leave_notify_event_cb(GtkWidget* /*widget*/, GdkEventCrossing* event,
                                      gpointer user_data) {
  FloatingBallWindow* self = (FloatingBallWindow*)user_data;
  if (self->destroying || event->mode != GDK_CROSSING_NORMAL ||
      self->dock_edge == kDockEdgeNone || self->collapsed ||
      self->pointer_down) {
    // Not docked, already collapsed, or a click/drag is in progress (item 6
    // handles drag-away separately) — no collapse countdown to arm.
    return FALSE;
  }
  cancel_collapse_delay(self);
  self->collapse_delay_id =
      g_timeout_add(kDockCollapseDelayMs, collapse_delay_cb, self);
  return FALSE;
}

// =============================================================================
// MethodChannel handlers
// =============================================================================

static FlMethodResponse* handle_query_capability(FloatingBallWindow* self) {
  send_capability(self);
  return FL_METHOD_RESPONSE(fl_method_success_response_new(nullptr));
}

static FlMethodResponse* handle_push_bitmap(FloatingBallWindow* self,
                                            FlValue* args) {
  if (args == nullptr || fl_value_get_type(args) != FL_VALUE_TYPE_MAP) {
    return FL_METHOD_RESPONSE(
        fl_method_error_response_new("bad_args", "pushBitmap requires a map", nullptr));
  }
  FlValue* bytes_value = fl_value_lookup_string(args, "bytes");
  FlValue* width_value = fl_value_lookup_string(args, "width");
  FlValue* height_value = fl_value_lookup_string(args, "height");
  FlValue* scale_value = fl_value_lookup_string(args, "scale");
  if (bytes_value == nullptr ||
      fl_value_get_type(bytes_value) != FL_VALUE_TYPE_UINT8_LIST ||
      width_value == nullptr ||
      fl_value_get_type(width_value) != FL_VALUE_TYPE_INT ||
      height_value == nullptr ||
      fl_value_get_type(height_value) != FL_VALUE_TYPE_INT ||
      scale_value == nullptr ||
      fl_value_get_type(scale_value) != FL_VALUE_TYPE_FLOAT) {
    return FL_METHOD_RESPONSE(fl_method_error_response_new(
        "bad_args", "pushBitmap: missing/invalid bytes|width|height|scale", nullptr));
  }

  const int width = (int)fl_value_get_int(width_value);
  const int height = (int)fl_value_get_int(height_value);
  const double scale = fl_value_get_float(scale_value);
  const size_t byte_len = fl_value_get_length(bytes_value);
  if (width <= 0 || height <= 0 || scale <= 0.0 ||
      byte_len != (size_t)width * (size_t)height * 4) {
    return FL_METHOD_RESPONSE(fl_method_error_response_new(
        "bad_args", "pushBitmap: dimensions do not match byte length", nullptr));
  }

  g_free(self->bitmap);
  self->bitmap = (guint8*)g_malloc(byte_len);
  memcpy(self->bitmap, fl_value_get_uint8_list(bytes_value), byte_len);
  self->bitmap_width = width;
  self->bitmap_height = height;
  self->bitmap_scale = scale;

  // Window's logical footprint = width/scale (GTK handles its own integer
  // scale-factor separately; this is Dart's devicePixelRatio). Normally a
  // stable 72px, but derived rather than hardcoded per the spec.
  const int logical = (int)lround(width / scale);
  if (self->window != nullptr && logical > 0 && logical != self->logical_size) {
    self->logical_size = logical;
    gtk_window_resize(GTK_WINDOW(self->window), logical, logical);
    apply_input_shape(self);
  }

  if (self->window != nullptr) {
    gtk_widget_queue_draw(self->window);
  }

  return FL_METHOD_RESPONSE(fl_method_success_response_new(nullptr));
}

static FlMethodResponse* handle_show_ball(FloatingBallWindow* self, FlValue* args) {
  if (!self->is_x11) {
    // Defensive: Dart gates showBall behind onCapability == "x11", so this
    // path should not be reachable under a real Wayland session.
    return FL_METHOD_RESPONSE(fl_method_success_response_new(nullptr));
  }

  double x = -1.0, y = -1.0;
  if (args != nullptr && fl_value_get_type(args) == FL_VALUE_TYPE_MAP) {
    FlValue* x_value = fl_value_lookup_string(args, "x");
    FlValue* y_value = fl_value_lookup_string(args, "y");
    if (x_value != nullptr && fl_value_get_type(x_value) == FL_VALUE_TYPE_FLOAT) {
      x = fl_value_get_float(x_value);
    }
    if (y_value != nullptr && fl_value_get_type(y_value) == FL_VALUE_TYPE_FLOAT) {
      y = fl_value_get_float(y_value);
    }
  }

  ensure_window(self);
  self->destroying = FALSE;

  gint out_x = 0, out_y = 0;
  resolve_position(self, x, y, &out_x, &out_y);
  gtk_window_move(GTK_WINDOW(self->window), out_x, out_y);
  gtk_widget_show(self->window);
  // Re-apply the input shape in case this is a hide->show cycle on an
  // already-realized window (the "realize" signal, which also applies the
  // shape, only fires once per widget lifetime), and immediately snap-judge
  // dock state: a restored position flush against a work-area edge (last
  // session's dock) must re-enter dock state right away, not 300ms+ later
  // via the drag-settle debounce (item 9).
  evaluate_dock_snap(self);

  return FL_METHOD_RESPONSE(fl_method_success_response_new(nullptr));
}

static FlMethodResponse* handle_hide_ball(FloatingBallWindow* self) {
  self->destroying = TRUE;
  cancel_move_debounce(self);
  cancel_dock_timeouts(self);
  if (self->window != nullptr) {
    gtk_widget_hide(self->window);
  }
  return FL_METHOD_RESPONSE(fl_method_success_response_new(nullptr));
}

static FlMethodResponse* handle_destroy_ball(FloatingBallWindow* self) {
  self->destroying = TRUE;
  cancel_move_debounce(self);
  cancel_dock_timeouts(self);
  if (self->window != nullptr) {
    gtk_widget_destroy(self->window);  // -> on_window_destroy() clears the pointer
  }
  g_free(self->bitmap);
  self->bitmap = nullptr;
  self->bitmap_width = 0;
  self->bitmap_height = 0;
  self->pointer_down = FALSE;
  self->dragging = FALSE;
  self->drag_hover = FALSE;
  self->dock_edge = kDockEdgeNone;
  self->collapsed = FALSE;
  return FL_METHOD_RESPONSE(fl_method_success_response_new(nullptr));
}

static FlMethodResponse* handle_show_context_menu(FloatingBallWindow* self,
                                                   FlValue* args) {
  if (self->window == nullptr) {
    // Ball not on screen (e.g. hidden between the right-click and this call
    // arriving) — nothing to pop the menu against.
    return FL_METHOD_RESPONSE(fl_method_success_response_new(nullptr));
  }
  if (args == nullptr || fl_value_get_type(args) != FL_VALUE_TYPE_MAP) {
    return FL_METHOD_RESPONSE(fl_method_error_response_new(
        "bad_args", "showContextMenu requires a map", nullptr));
  }
  FlValue* items = fl_value_lookup_string(args, "items");
  if (items == nullptr || fl_value_get_type(items) != FL_VALUE_TYPE_LIST) {
    return FL_METHOD_RESPONSE(fl_method_error_response_new(
        "bad_args", "showContextMenu: missing/invalid items", nullptr));
  }

  GtkWidget* menu = gtk_menu_new();
  // Sink the floating ref so we own a normal ref past the popup call;
  // context_menu_deactivate_cb() drops it once the popup closes.
  g_object_ref_sink(menu);

  const size_t count = fl_value_get_length(items);
  for (size_t i = 0; i < count; i++) {
    FlValue* item = fl_value_get_list_value(items, i);
    if (item == nullptr || fl_value_get_type(item) != FL_VALUE_TYPE_MAP) {
      continue;
    }
    FlValue* id_value = fl_value_lookup_string(item, "id");
    const int id = (id_value != nullptr &&
                    fl_value_get_type(id_value) == FL_VALUE_TYPE_INT)
                       ? (int)fl_value_get_int(id_value)
                       : 0;

    GtkWidget* menu_item;
    if (id == 0) {
      menu_item = gtk_separator_menu_item_new();
    } else {
      FlValue* label_value = fl_value_lookup_string(item, "label");
      const gchar* label =
          (label_value != nullptr &&
          fl_value_get_type(label_value) == FL_VALUE_TYPE_STRING)
              ? fl_value_get_string(label_value)
              : "";
      menu_item = gtk_menu_item_new_with_label(label);
      g_object_set_data(G_OBJECT(menu_item), "fluxdown-menu-item-id",
                        GINT_TO_POINTER(id));
      g_signal_connect(menu_item, "activate",
                       G_CALLBACK(context_menu_item_activate_cb), self);
    }
    gtk_menu_shell_append(GTK_MENU_SHELL(menu), menu_item);
  }

  g_signal_connect(menu, "deactivate", G_CALLBACK(context_menu_deactivate_cb),
                   nullptr);
  gtk_widget_show_all(menu);
  gtk_menu_popup_at_pointer(GTK_MENU(menu), nullptr);

  return FL_METHOD_RESPONSE(fl_method_success_response_new(nullptr));
}

static void method_call_cb(FlMethodChannel* /*channel*/, FlMethodCall* method_call,
                           gpointer user_data) {
  FloatingBallWindow* self = (FloatingBallWindow*)user_data;
  const gchar* method = fl_method_call_get_name(method_call);
  FlValue* args = fl_method_call_get_args(method_call);

  g_autoptr(FlMethodResponse) response = nullptr;
  if (g_strcmp0(method, "queryCapability") == 0) {
    response = handle_query_capability(self);
  } else if (g_strcmp0(method, "pushBitmap") == 0) {
    response = handle_push_bitmap(self, args);
  } else if (g_strcmp0(method, "showBall") == 0) {
    response = handle_show_ball(self, args);
  } else if (g_strcmp0(method, "hideBall") == 0) {
    response = handle_hide_ball(self);
  } else if (g_strcmp0(method, "destroyBall") == 0) {
    response = handle_destroy_ball(self);
  } else if (g_strcmp0(method, "showContextMenu") == 0) {
    response = handle_show_context_menu(self, args);
  } else {
    response = FL_METHOD_RESPONSE(fl_method_not_implemented_response_new());
  }

  g_autoptr(GError) error = nullptr;
  if (!fl_method_call_respond(method_call, response, &error)) {
    g_warning("Failed to send floating_ball response: %s", error->message);
  }
}

// =============================================================================
// Public API
// =============================================================================

FloatingBallWindow* floating_ball_window_new(FlBinaryMessenger* messenger) {
  FloatingBallWindow* self = g_new0(FloatingBallWindow, 1);
  self->is_x11 = detect_is_x11();
  self->logical_size = kDefaultLogicalSize;
  self->bitmap_scale = 1.0;

  g_autoptr(FlStandardMethodCodec) codec = fl_standard_method_codec_new();
  self->channel = fl_method_channel_new(messenger, "com.fluxdown/floating_ball",
                                        FL_METHOD_CODEC(codec));
  fl_method_channel_set_method_call_handler(self->channel, method_call_cb, self,
                                            nullptr);
  return self;
}

void floating_ball_window_free(FloatingBallWindow* self) {
  if (self == nullptr) {
    return;
  }
  self->destroying = TRUE;
  cancel_move_debounce(self);
  cancel_dock_timeouts(self);
  if (self->window != nullptr) {
    gtk_widget_destroy(self->window);
  }
  if (self->channel != nullptr) {
    fl_method_channel_set_method_call_handler(self->channel, nullptr, nullptr,
                                              nullptr);
    g_object_unref(self->channel);
  }
  g_free(self->bitmap);
  g_free(self);
}
