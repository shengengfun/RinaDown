// tray_manager_plugin.cc — Linux StatusNotifierItem + com.canonical.dbusmenu
//
// Replaces the AppIndicator backend with a native D-Bus SNI implementation
// so that KDE Plasma (Wayland & X11) can:
//   - Call Activate  → left-click  → emits onTrayIconMouseDown to Flutter
//   - Read dbusmenu  → right-click → Flutter menu items rendered by Plasma
//
// Dependencies: gio-2.0, gdk-pixbuf-2.0 (both pulled transitively by GTK3).

#include "include/tray_manager/tray_manager_plugin.h"

#include <flutter_linux/flutter_linux.h>
#include <gdk-pixbuf/gdk-pixbuf.h>
#include <gio/gio.h>
#include <gtk/gtk.h>
#include <unistd.h>

#include <cstdio>
#include <cstring>
#include <string>
#include <vector>

// ─── Plugin struct ────────────────────────────────────────────────────────────

#define TRAY_MANAGER_PLUGIN(obj)                                     \
  (G_TYPE_CHECK_INSTANCE_CAST((obj), tray_manager_plugin_get_type(), \
                              TrayManagerPlugin))

struct _TrayManagerPlugin {
  GObject parent_instance;
  FlPluginRegistrar* registrar;
  FlMethodChannel* channel;
};

G_DEFINE_TYPE(TrayManagerPlugin, tray_manager_plugin, g_object_get_type())

// ─── D-Bus constants ──────────────────────────────────────────────────────────

#define SNI_WATCHER_BUS   "org.kde.StatusNotifierWatcher"
#define SNI_WATCHER_PATH  "/StatusNotifierWatcher"
#define SNI_WATCHER_IFACE "org.kde.StatusNotifierWatcher"
#define SNI_ITEM_PATH     "/StatusNotifierItem"
#define SNI_ITEM_IFACE    "org.kde.StatusNotifierItem"
#define DBUSMENU_PATH     "/MenuBar"
#define DBUSMENU_IFACE    "com.canonical.dbusmenu"

// Stable application identifier exposed as StatusNotifierItem.Id. It must not
// change at runtime: hosts derive their per-item identity from it.
#define SNI_APP_ID        "FluxDown"

// ─── Menu entry ───────────────────────────────────────────────────────────────

struct MenuEntry {
  int         id;
  std::string type;      // "normal", "separator", "checkbox"
  std::string label;
  bool        disabled;
  bool        checked;
};

// ─── Module-level state ───────────────────────────────────────────────────────

static TrayManagerPlugin*  s_plugin    = nullptr;
static GDBusConnection*    s_conn      = nullptr;
static guint               s_own_id    = 0;
static guint               s_sni_reg   = 0;
static guint               s_menu_reg  = 0;
static bool                s_active    = false;
static std::string         s_bus_name;
static guint               s_watcher_id = 0;
static bool                s_name_acquired  = false;
static bool                s_watcher_present = false;
static std::string         s_icon_path;
static std::string         s_title     = "FluxDown";
static std::vector<MenuEntry> s_menu_items;
static guint32             s_menu_rev  = 1;

// ─── Emit event to Flutter ────────────────────────────────────────────────────

static void flutter_emit(const char* method, FlValue* args) {
  if (!s_plugin || !s_plugin->channel) return;
  g_autoptr(FlValue) empty = fl_value_new_map();
  fl_method_channel_invoke_method(s_plugin->channel, method,
                                  args ? args : empty,
                                  nullptr, nullptr, nullptr);
}

// ─── Icon: PNG → SNI ARGB pixmap ─────────────────────────────────────────────

static GVariant* make_icon_pixmap() {
  if (s_icon_path.empty()) {
    return g_variant_new_array(G_VARIANT_TYPE("(iiay)"), nullptr, 0);
  }

  GError* err = nullptr;
  GdkPixbuf* pb = gdk_pixbuf_new_from_file(s_icon_path.c_str(), &err);
  if (!pb) {
    if (err) g_error_free(err);
    return g_variant_new_array(G_VARIANT_TYPE("(iiay)"), nullptr, 0);
  }

  // Ensure the pixbuf has an alpha channel.
  GdkPixbuf* rgba = pb;
  if (!gdk_pixbuf_get_has_alpha(pb)) {
    rgba = gdk_pixbuf_add_alpha(pb, FALSE, 0, 0, 0);
    g_object_unref(pb);
  }

  int w          = gdk_pixbuf_get_width(rgba);
  int h          = gdk_pixbuf_get_height(rgba);
  int rowstride  = gdk_pixbuf_get_rowstride(rgba);
  guchar* pixels = gdk_pixbuf_get_pixels(rgba);
  int n_ch       = gdk_pixbuf_get_n_channels(rgba);  // 4 after add_alpha

  // Convert RGBA rows → ARGB byte stream (SNI network byte order).
  GVariantBuilder bytes;
  g_variant_builder_init(&bytes, G_VARIANT_TYPE("ay"));
  for (int y = 0; y < h; y++) {
    guchar* row = pixels + y * rowstride;
    for (int x = 0; x < w; x++) {
      guchar r = row[0], g = row[1], b = row[2], a = row[3];
      g_variant_builder_add(&bytes, "y", a);
      g_variant_builder_add(&bytes, "y", r);
      g_variant_builder_add(&bytes, "y", g);
      g_variant_builder_add(&bytes, "y", b);
      row += n_ch;
    }
  }
  g_object_unref(rgba);

  GVariantBuilder entry;
  g_variant_builder_init(&entry, G_VARIANT_TYPE("(iiay)"));
  g_variant_builder_add(&entry, "i", (gint32)w);
  g_variant_builder_add(&entry, "i", (gint32)h);
  g_variant_builder_add_value(&entry, g_variant_builder_end(&bytes));

  GVariantBuilder arr;
  g_variant_builder_init(&arr, G_VARIANT_TYPE("a(iiay)"));
  g_variant_builder_add_value(&arr, g_variant_builder_end(&entry));
  return g_variant_builder_end(&arr);
}

// ─── SNI D-Bus interface ──────────────────────────────────────────────────────

static const char SNI_XML[] =
    "<!DOCTYPE node PUBLIC"
    " \"-//freedesktop//DTD D-BUS Object Introspection 1.0//EN\""
    " \"http://www.freedesktop.org/standards/dbus/1.0/introspect.dtd\">\n"
    "<node>\n"
    "  <interface name='org.kde.StatusNotifierItem'>\n"
    "    <property name='Id'         type='s'            access='read'/>\n"
    "    <property name='Category'   type='s'            access='read'/>\n"
    "    <property name='Status'     type='s'            access='read'/>\n"
    "    <property name='Title'      type='s'            access='read'/>\n"
    "    <property name='IconName'   type='s'            access='read'/>\n"
    "    <property name='IconPixmap' type='a(iiay)'      access='read'/>\n"
    "    <property name='ToolTip'    type='(sa(iiay)ss)' access='read'/>\n"
    "    <property name='Menu'       type='o'            access='read'/>\n"
    "    <property name='ItemIsMenu' type='b'            access='read'/>\n"
    "    <method name='Activate'>\n"
    "      <arg type='i' direction='in' name='x'/>\n"
    "      <arg type='i' direction='in' name='y'/>\n"
    "    </method>\n"
    "    <method name='SecondaryActivate'>\n"
    "      <arg type='i' direction='in' name='x'/>\n"
    "      <arg type='i' direction='in' name='y'/>\n"
    "    </method>\n"
    "    <method name='ContextMenu'>\n"
    "      <arg type='i' direction='in' name='x'/>\n"
    "      <arg type='i' direction='in' name='y'/>\n"
    "    </method>\n"
    "    <signal name='NewIcon'/>\n"
    "    <signal name='NewTitle'/>\n"
    "    <signal name='NewStatus'><arg type='s' name='status'/></signal>\n"
    "    <signal name='NewToolTip'/>\n"
    "  </interface>\n"
    "</node>\n";

static void sni_method_call(GDBusConnection*, const gchar*, const gchar*,
                            const gchar*, const gchar* method_name,
                            GVariant*, GDBusMethodInvocation* inv,
                            gpointer) {
  if (strcmp(method_name, "Activate") == 0 ||
      strcmp(method_name, "SecondaryActivate") == 0) {
    // Left-click (or middle-click) → show the window.
    flutter_emit("onTrayIconMouseDown", nullptr);
    g_dbus_method_invocation_return_value(inv, nullptr);
  } else if (strcmp(method_name, "ContextMenu") == 0) {
    // KDE reads the dbusmenu automatically — just ACK.
    g_dbus_method_invocation_return_value(inv, nullptr);
  } else {
    g_dbus_method_invocation_return_error(
        inv, G_DBUS_ERROR, G_DBUS_ERROR_UNKNOWN_METHOD,
        "Unknown SNI method: %s", method_name);
  }
}

static GVariant* sni_get_property(GDBusConnection*, const gchar*,
                                  const gchar*, const gchar*,
                                  const gchar* prop, GError** error,
                                  gpointer) {
  // GNOME's AppIndicator host treats Id and Menu as the properties that gate
  // item readiness: without Id it retries a few times, then drops the icon.
  if (strcmp(prop, "Id") == 0)
    return g_variant_new_string(SNI_APP_ID);
  if (strcmp(prop, "Category") == 0)
    return g_variant_new_string("ApplicationStatus");
  if (strcmp(prop, "Status") == 0)
    return g_variant_new_string(s_active ? "Active" : "Passive");
  if (strcmp(prop, "Title") == 0)
    return g_variant_new_string(s_title.c_str());
  if (strcmp(prop, "IconName") == 0)
    return g_variant_new_string("");  // use IconPixmap instead
  if (strcmp(prop, "IconPixmap") == 0)
    return make_icon_pixmap();
  if (strcmp(prop, "ToolTip") == 0) {
    // (icon_name, icon_pixmap_array, title, body)
    GVariant* empty_pix =
        g_variant_new_array(G_VARIANT_TYPE("(iiay)"), nullptr, 0);
    return g_variant_new("(s@a(iiay)ss)", "", empty_pix, s_title.c_str(), "");
  }
  if (strcmp(prop, "Menu") == 0)
    return g_variant_new_object_path(DBUSMENU_PATH);
  if (strcmp(prop, "ItemIsMenu") == 0)
    // FALSE → Plasma calls Activate on left-click (not ContextMenu).
    return g_variant_new_boolean(FALSE);
  // GDBus requires the error to be set whenever NULL is returned.
  g_set_error(error, G_DBUS_ERROR, G_DBUS_ERROR_INVALID_ARGS,
              "No such property: %s", prop);
  return nullptr;
}

static const GDBusInterfaceVTable sni_vtable = {
    sni_method_call, sni_get_property, nullptr};

// ─── dbusmenu D-Bus interface ─────────────────────────────────────────────────

static const char MENU_XML[] =
    "<!DOCTYPE node PUBLIC"
    " \"-//freedesktop//DTD D-BUS Object Introspection 1.0//EN\""
    " \"http://www.freedesktop.org/standards/dbus/1.0/introspect.dtd\">\n"
    "<node>\n"
    "  <interface name='com.canonical.dbusmenu'>\n"
    "    <property name='Version'       type='u'  access='read'/>\n"
    "    <property name='TextDirection' type='s'  access='read'/>\n"
    "    <property name='Status'        type='s'  access='read'/>\n"
    "    <property name='IconThemePath' type='as' access='read'/>\n"
    "    <method name='GetLayout'>\n"
    "      <arg type='i'          direction='in'  name='parentId'/>\n"
    "      <arg type='i'          direction='in'  name='recursionDepth'/>\n"
    "      <arg type='as'         direction='in'  name='propertyNames'/>\n"
    "      <arg type='u'          direction='out' name='revision'/>\n"
    "      <arg type='(ia{sv}av)' direction='out' name='layout'/>\n"
    "    </method>\n"
    "    <method name='GetGroupProperties'>\n"
    "      <arg type='ai'        direction='in'  name='ids'/>\n"
    "      <arg type='as'        direction='in'  name='propertyNames'/>\n"
    "      <arg type='a(ia{sv})' direction='out' name='properties'/>\n"
    "    </method>\n"
    "    <method name='GetProperty'>\n"
    "      <arg type='i' direction='in'  name='id'/>\n"
    "      <arg type='s' direction='in'  name='name'/>\n"
    "      <arg type='v' direction='out' name='value'/>\n"
    "    </method>\n"
    "    <method name='Event'>\n"
    "      <arg type='i' direction='in' name='id'/>\n"
    "      <arg type='s' direction='in' name='eventId'/>\n"
    "      <arg type='v' direction='in' name='data'/>\n"
    "      <arg type='u' direction='in' name='timestamp'/>\n"
    "    </method>\n"
    "    <method name='EventGroup'>\n"
    "      <arg type='a(isvu)' direction='in'  name='events'/>\n"
    "      <arg type='ai'      direction='out' name='idErrors'/>\n"
    "    </method>\n"
    "    <method name='AboutToShow'>\n"
    "      <arg type='i' direction='in'  name='id'/>\n"
    "      <arg type='b' direction='out' name='needUpdate'/>\n"
    "    </method>\n"
    "    <method name='AboutToShowGroup'>\n"
    "      <arg type='ai' direction='in'  name='ids'/>\n"
    "      <arg type='ai' direction='out' name='updatesNeeded'/>\n"
    "      <arg type='ai' direction='out' name='idErrors'/>\n"
    "    </method>\n"
    "    <signal name='ItemsPropertiesUpdated'>\n"
    "      <arg type='a(ia{sv})' name='updatedProps'/>\n"
    "      <arg type='a(ias)'    name='removedProps'/>\n"
    "    </signal>\n"
    "    <signal name='LayoutUpdated'>\n"
    "      <arg type='u' name='revision'/>\n"
    "      <arg type='i' name='parent'/>\n"
    "    </signal>\n"
    "    <signal name='ItemActivationRequested'>\n"
    "      <arg type='i' name='id'/>\n"
    "      <arg type='u' name='timestamp'/>\n"
    "    </signal>\n"
    "  </interface>\n"
    "</node>\n";

// Build a single menu item as GVariant type (ia{sv}av).
static GVariant* build_menu_item(const MenuEntry& e) {
  GVariantBuilder props;
  g_variant_builder_init(&props, G_VARIANT_TYPE("a{sv}"));

  if (e.type == "separator") {
    g_variant_builder_add(&props, "{sv}", "type",
                          g_variant_new_string("separator"));
  } else {
    g_variant_builder_add(&props, "{sv}", "label",
                          g_variant_new_string(e.label.c_str()));
    g_variant_builder_add(&props, "{sv}", "enabled",
                          g_variant_new_boolean(!e.disabled));
    g_variant_builder_add(&props, "{sv}", "visible",
                          g_variant_new_boolean(TRUE));
    if (e.type == "checkbox") {
      g_variant_builder_add(&props, "{sv}", "toggle-type",
                            g_variant_new_string("checkmark"));
      g_variant_builder_add(&props, "{sv}", "toggle-state",
                            g_variant_new_int32(e.checked ? 1 : 0));
    }
  }

  // No sub-children for these items.
  GVariantBuilder children;
  g_variant_builder_init(&children, G_VARIANT_TYPE("av"));

  return g_variant_new("(i@a{sv}@av)", (gint32)e.id,
                       g_variant_builder_end(&props),
                       g_variant_builder_end(&children));
}

// Build root layout (ia{sv}av) where children hold all menu items.
static GVariant* build_root_layout() {
  GVariantBuilder props;
  g_variant_builder_init(&props, G_VARIANT_TYPE("a{sv}"));
  g_variant_builder_add(&props, "{sv}", "children-display",
                        g_variant_new_string("submenu"));

  GVariantBuilder children;
  g_variant_builder_init(&children, G_VARIANT_TYPE("av"));
  for (const auto& item : s_menu_items) {
    // Pass the floating ref directly — g_variant_builder_add consumes it
    // via g_variant_new_variant() internally (sinks the floating ref).
    // Do NOT use g_autoptr here: that would double-free the ref.
    g_variant_builder_add(&children, "v", build_menu_item(item));
  }

  return g_variant_new("(i@a{sv}@av)", (gint32)0,
                       g_variant_builder_end(&props),
                       g_variant_builder_end(&children));
}

static void fire_menu_click(gint32 id) {
  g_autoptr(FlValue) args = fl_value_new_map();
  fl_value_set_string_take(args, "id", fl_value_new_int(id));
  flutter_emit("onTrayMenuItemClick", args);
}

static void menu_method_call(GDBusConnection*, const gchar*,
                             const gchar*, const gchar*,
                             const gchar* method_name, GVariant* params,
                             GDBusMethodInvocation* inv, gpointer) {
  if (strcmp(method_name, "GetLayout") == 0) {
    // build_root_layout() returns a floating ref. g_variant_new with "@"
    // format sinks it (takes ownership). Do NOT use g_autoptr here:
    // that would unref after the sink and double-free the value.
    g_dbus_method_invocation_return_value(
        inv, g_variant_new("(u@(ia{sv}av))", s_menu_rev, build_root_layout()));

  } else if (strcmp(method_name, "AboutToShow") == 0) {
    g_dbus_method_invocation_return_value(inv, g_variant_new("(b)", FALSE));

  } else if (strcmp(method_name, "AboutToShowGroup") == 0) {
    GVariant* e1 = g_variant_new_array(G_VARIANT_TYPE("i"), nullptr, 0);
    GVariant* e2 = g_variant_new_array(G_VARIANT_TYPE("i"), nullptr, 0);
    g_dbus_method_invocation_return_value(
        inv, g_variant_new("(@ai@ai)", e1, e2));

  } else if (strcmp(method_name, "Event") == 0) {
    gint32 id = 0;
    const gchar* event_id = nullptr;
    GVariant* data = nullptr;
    guint32 ts = 0;
    g_variant_get(params, "(i&s@vu)", &id, &event_id, &data, &ts);
    if (data) g_variant_unref(data);

    if (event_id && strcmp(event_id, "clicked") == 0) {
      fire_menu_click(id);
    }
    g_dbus_method_invocation_return_value(inv, nullptr);

  } else if (strcmp(method_name, "EventGroup") == 0) {
    GVariantIter* iter = nullptr;
    g_variant_get(params, "(a(isvu))", &iter);
    if (iter) {
      gint32 id = 0;
      gchar* event_id = nullptr;
      GVariant* data = nullptr;
      guint32 ts = 0;
      while (g_variant_iter_loop(iter, "(is@vu)", &id, &event_id, &data, &ts)) {
        if (event_id && strcmp(event_id, "clicked") == 0) {
          fire_menu_click(id);
        }
      }
      g_variant_iter_free(iter);
    }
    GVariant* empty = g_variant_new_array(G_VARIANT_TYPE("i"), nullptr, 0);
    g_dbus_method_invocation_return_value(inv, g_variant_new("(@ai)", empty));

  } else if (strcmp(method_name, "GetGroupProperties") == 0) {
    GVariant* empty =
        g_variant_new_array(G_VARIANT_TYPE("(ia{sv})"), nullptr, 0);
    g_dbus_method_invocation_return_value(
        inv, g_variant_new("(@a(ia{sv}))", empty));

  } else if (strcmp(method_name, "GetProperty") == 0) {
    g_dbus_method_invocation_return_value(
        inv, g_variant_new("(v)", g_variant_new_string("")));

  } else {
    g_dbus_method_invocation_return_error(
        inv, G_DBUS_ERROR, G_DBUS_ERROR_UNKNOWN_METHOD,
        "Unknown dbusmenu method: %s", method_name);
  }
}

static GVariant* menu_get_property(GDBusConnection*, const gchar*,
                                   const gchar*, const gchar*,
                                   const gchar* prop, GError**, gpointer) {
  if (strcmp(prop, "Version") == 0)       return g_variant_new_uint32(3);
  if (strcmp(prop, "TextDirection") == 0) return g_variant_new_string("ltr");
  if (strcmp(prop, "Status") == 0)        return g_variant_new_string("normal");
  if (strcmp(prop, "IconThemePath") == 0) return g_variant_new_strv(nullptr, 0);
  return nullptr;
}

static const GDBusInterfaceVTable menu_vtable = {
    menu_method_call, menu_get_property, nullptr};

// ─── D-Bus signal helpers ─────────────────────────────────────────────────────

static void emit_signal(const char* path, const char* iface,
                        const char* signal_name, GVariant* params) {
  if (!s_conn) return;
  g_dbus_connection_emit_signal(s_conn, nullptr, path, iface, signal_name,
                                params, nullptr);
}

static void notify_new_icon() {
  emit_signal(SNI_ITEM_PATH, SNI_ITEM_IFACE, "NewIcon", nullptr);
}

static void notify_layout_updated() {
  s_menu_rev++;
  emit_signal(DBUSMENU_PATH, DBUSMENU_IFACE, "LayoutUpdated",
              g_variant_new("(ui)", s_menu_rev, 0));
}

// ─── D-Bus bus ownership callbacks ───────────────────────────────────────────

static void on_bus_acquired(GDBusConnection* conn, const gchar*, gpointer) {
  s_conn = conn;
  GError* err = nullptr;

  g_autoptr(GDBusNodeInfo) sni_info =
      g_dbus_node_info_new_for_xml(SNI_XML, &err);
  if (err) {
    g_warning("tray_manager: SNI XML parse error: %s", err->message);
    g_clear_error(&err);
  }

  g_autoptr(GDBusNodeInfo) menu_info =
      g_dbus_node_info_new_for_xml(MENU_XML, &err);
  if (err) {
    g_warning("tray_manager: Menu XML parse error: %s", err->message);
    g_clear_error(&err);
  }

  if (sni_info && sni_info->interfaces[0]) {
    s_sni_reg = g_dbus_connection_register_object(
        conn, SNI_ITEM_PATH, sni_info->interfaces[0],
        &sni_vtable, nullptr, nullptr, &err);
    if (err) {
      g_warning("tray_manager: SNI register error: %s", err->message);
      g_clear_error(&err);
    }
  }

  if (menu_info && menu_info->interfaces[0]) {
    s_menu_reg = g_dbus_connection_register_object(
        conn, DBUSMENU_PATH, menu_info->interfaces[0],
        &menu_vtable, nullptr, nullptr, &err);
    if (err) {
      g_warning("tray_manager: Menu register error: %s", err->message);
      g_clear_error(&err);
    }
  }
}

static void on_register_finished(GObject* source, GAsyncResult* res,
                                 gpointer) {
  GError* err = nullptr;
  GVariant* ret =
      g_dbus_connection_call_finish(G_DBUS_CONNECTION(source), res, &err);
  if (ret) g_variant_unref(ret);
  if (err) {
    g_warning("tray_manager: RegisterStatusNotifierItem failed: %s",
              err->message);
    g_error_free(err);
  }
}

// Registration is redone every time the watcher shows up: the host may start
// after us, be restarted, or have its tray support toggled. A one-shot call
// at name-acquired time would lose the icon permanently in those cases.
static void register_with_watcher() {
  if (!s_conn || !s_name_acquired || !s_watcher_present) return;
  g_dbus_connection_call(s_conn,
                         SNI_WATCHER_BUS, SNI_WATCHER_PATH, SNI_WATCHER_IFACE,
                         "RegisterStatusNotifierItem",
                         g_variant_new("(s)", s_bus_name.c_str()),
                         nullptr, G_DBUS_CALL_FLAGS_NONE,
                         -1, nullptr, on_register_finished, nullptr);
}

static void on_watcher_appeared(GDBusConnection*, const gchar*,
                                const gchar*, gpointer) {
  s_watcher_present = true;
  register_with_watcher();
}

static void on_watcher_vanished(GDBusConnection*, const gchar*, gpointer) {
  s_watcher_present = false;
}

static void on_name_acquired(GDBusConnection*, const gchar*, gpointer) {
  s_name_acquired = true;
  register_with_watcher();
}

static void on_name_lost(GDBusConnection*, const gchar*, gpointer) {
  s_name_acquired = false;
  g_warning("tray_manager: failed to acquire StatusNotifierItem bus name");
}

// ─── D-Bus initialisation ────────────────────────────────────────────────────

static void ensure_dbus_initialized() {
  if (s_own_id != 0) return;
  s_active = true;

  // Bus name format required by the SNI spec.
  gchar* bus_name =
      g_strdup_printf("org.kde.StatusNotifierItem-%d-1", (int)getpid());
  s_bus_name = bus_name;
  s_own_id = g_bus_own_name(G_BUS_TYPE_SESSION, bus_name,
                             G_BUS_NAME_OWNER_FLAGS_NONE,
                             on_bus_acquired, on_name_acquired, on_name_lost,
                             nullptr, nullptr);
  g_free(bus_name);

  s_watcher_id = g_bus_watch_name(G_BUS_TYPE_SESSION, SNI_WATCHER_BUS,
                                  G_BUS_NAME_WATCHER_FLAGS_NONE,
                                  on_watcher_appeared, on_watcher_vanished,
                                  nullptr, nullptr);
}

// ─── Flutter method handlers ─────────────────────────────────────────────────

static FlMethodResponse* handle_set_icon(FlValue* args) {
  FlValue* v = fl_value_lookup_string(args, "iconPath");
  s_icon_path = (v && fl_value_get_type(v) == FL_VALUE_TYPE_STRING)
                    ? fl_value_get_string(v)
                    : "";
  ensure_dbus_initialized();
  if (s_conn) notify_new_icon();
  return FL_METHOD_RESPONSE(
      fl_method_success_response_new(fl_value_new_bool(true)));
}

static FlMethodResponse* handle_set_title(FlValue* args) {
  FlValue* v = fl_value_lookup_string(args, "title");
  s_title = (v && fl_value_get_type(v) == FL_VALUE_TYPE_STRING)
                ? fl_value_get_string(v)
                : "";
  if (s_conn)
    emit_signal(SNI_ITEM_PATH, SNI_ITEM_IFACE, "NewTitle", nullptr);
  return FL_METHOD_RESPONSE(
      fl_method_success_response_new(fl_value_new_bool(true)));
}

static FlMethodResponse* handle_set_context_menu(FlValue* args) {
  s_menu_items.clear();

  FlValue* menu_val = fl_value_lookup_string(args, "menu");
  if (!menu_val) goto done;

  {
    FlValue* items_val = fl_value_lookup_string(menu_val, "items");
    if (!items_val) goto done;

    int n = fl_value_get_length(items_val);
    for (int i = 0; i < n; i++) {
      FlValue* item = fl_value_get_list_value(items_val, i);
      MenuEntry e;

      FlValue* id_v = fl_value_lookup_string(item, "id");
      e.id = id_v ? (int)fl_value_get_int(id_v) : i;

      FlValue* type_v = fl_value_lookup_string(item, "type");
      e.type = (type_v && fl_value_get_type(type_v) == FL_VALUE_TYPE_STRING)
                   ? fl_value_get_string(type_v)
                   : "normal";

      FlValue* label_v = fl_value_lookup_string(item, "label");
      e.label = (label_v && fl_value_get_type(label_v) == FL_VALUE_TYPE_STRING)
                    ? fl_value_get_string(label_v)
                    : "";

      FlValue* dis_v = fl_value_lookup_string(item, "disabled");
      e.disabled = dis_v && fl_value_get_type(dis_v) == FL_VALUE_TYPE_BOOL &&
                   fl_value_get_bool(dis_v);

      FlValue* chk_v = fl_value_lookup_string(item, "checked");
      e.checked = chk_v && fl_value_get_type(chk_v) == FL_VALUE_TYPE_BOOL &&
                  fl_value_get_bool(chk_v);

      s_menu_items.push_back(e);
    }
  }

done:
  if (s_conn) notify_layout_updated();
  return FL_METHOD_RESPONSE(
      fl_method_success_response_new(fl_value_new_bool(true)));
}

static FlMethodResponse* handle_destroy() {
  s_active = false;
  if (s_watcher_id) {
    g_bus_unwatch_name(s_watcher_id);
    s_watcher_id = 0;
  }
  s_watcher_present = false;
  s_name_acquired = false;
  if (s_conn) {
    if (s_sni_reg) {
      g_dbus_connection_unregister_object(s_conn, s_sni_reg);
      s_sni_reg = 0;
    }
    if (s_menu_reg) {
      g_dbus_connection_unregister_object(s_conn, s_menu_reg);
      s_menu_reg = 0;
    }
    s_conn = nullptr;
  }
  if (s_own_id) {
    g_bus_unown_name(s_own_id);
    s_own_id = 0;
  }
  s_bus_name.clear();
  return FL_METHOD_RESPONSE(
      fl_method_success_response_new(fl_value_new_bool(true)));
}

// ─── Plugin dispatch ──────────────────────────────────────────────────────────

static void plugin_handle_method_call(TrayManagerPlugin* self,
                                      FlMethodCall* call) {
  (void)self;
  g_autoptr(FlMethodResponse) response = nullptr;
  const gchar* method = fl_method_call_get_name(call);
  FlValue* args = fl_method_call_get_args(call);

  if (strcmp(method, "setIcon") == 0) {
    response = handle_set_icon(args);
  } else if (strcmp(method, "setTitle") == 0) {
    response = handle_set_title(args);
  } else if (strcmp(method, "setContextMenu") == 0) {
    response = handle_set_context_menu(args);
  } else if (strcmp(method, "destroy") == 0) {
    response = handle_destroy();
  } else {
    // setToolTip, popUpContextMenu, getBounds → no-op on Linux SNI.
    response = FL_METHOD_RESPONSE(
        fl_method_success_response_new(fl_value_new_bool(false)));
  }

  fl_method_call_respond(call, response, nullptr);
}

// ─── Plugin boilerplate ───────────────────────────────────────────────────────

static void tray_manager_plugin_dispose(GObject* object) {
  G_OBJECT_CLASS(tray_manager_plugin_parent_class)->dispose(object);
}

static void tray_manager_plugin_class_init(TrayManagerPluginClass* klass) {
  G_OBJECT_CLASS(klass)->dispose = tray_manager_plugin_dispose;
}

static void tray_manager_plugin_init(TrayManagerPlugin*) {}

static void method_call_cb(FlMethodChannel*, FlMethodCall* call,
                           gpointer user_data) {
  plugin_handle_method_call(TRAY_MANAGER_PLUGIN(user_data), call);
}

void tray_manager_plugin_register_with_registrar(
    FlPluginRegistrar* registrar) {
  TrayManagerPlugin* plugin = TRAY_MANAGER_PLUGIN(
      g_object_new(tray_manager_plugin_get_type(), nullptr));
  plugin->registrar = FL_PLUGIN_REGISTRAR(g_object_ref(registrar));

  g_autoptr(FlStandardMethodCodec) codec = fl_standard_method_codec_new();
  plugin->channel =
      fl_method_channel_new(fl_plugin_registrar_get_messenger(registrar),
                            "tray_manager", FL_METHOD_CODEC(codec));
  fl_method_channel_set_method_call_handler(plugin->channel, method_call_cb,
                                            g_object_ref(plugin),
                                            g_object_unref);
  s_plugin = plugin;
  g_object_unref(plugin);
}
