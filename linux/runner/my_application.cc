#include "my_application.h"

#include <flutter_linux/flutter_linux.h>
#ifdef GDK_WINDOWING_X11
#include <gdk/gdkx.h>
#endif

#include "flutter/generated_plugin_registrant.h"

#include "floating_ball_window.h"
#include "popup_window_host.h"

struct _MyApplication {
  GtkApplication parent_instance;
  char** dart_entrypoint_arguments;
  // Stored after the FlView is created; used in my_application_open() to reach
  // the MethodChannel when a second instance opens files.  Not owned here —
  // the GTK widget tree owns the FlView.
  FlView* view;
  // Native controller for the com.fluxdown/floating_ball channel (S3.4/A6).
  // Created once in my_application_activate() and owned here.
  FloatingBallWindow* floating_ball;
  // 外部唤起下载小窗的原生宿主（跨端弹窗契约 v1）。同样在
  // my_application_activate() 里创建一次并持有；承载第二个 Flutter 引擎的
  // 弹窗窗口本身懒创建，首次收到 show 请求时才真正建出来。
  PopupWindowHost* popup_host;
};

G_DEFINE_TYPE(MyApplication, my_application, GTK_TYPE_APPLICATION)

// Called when first Flutter frame received.
static void first_frame_cb(MyApplication* self, FlView* view) {
  // Skip showing the window if launched with --silentStart (boot autostart).
  gchar** args = self->dart_entrypoint_arguments;
  if (args != nullptr) {
    for (int i = 0; args[i] != nullptr; i++) {
      if (g_strcmp0(args[i], "--silentStart") == 0) {
        return;
      }
    }
  }
  gtk_widget_show(gtk_widget_get_toplevel(GTK_WIDGET(view)));
}

// Implements GApplication::activate.
static void my_application_activate(GApplication* application) {
  MyApplication* self = MY_APPLICATION(application);

  // If a window already exists (second launch), present it and return.
  GList* windows = gtk_application_get_windows(GTK_APPLICATION(application));
  if (windows != nullptr) {
    gtk_window_present(GTK_WINDOW(windows->data));
    return;
  }

  GtkWindow* window =
      GTK_WINDOW(gtk_application_window_new(GTK_APPLICATION(application)));

  // Use a header bar when running in GNOME as this is the common style used
  // by applications and is the setup most users will be using (e.g. Ubuntu
  // desktop).
  // If running on X and not using GNOME then just use a traditional title bar
  // in case the window manager does more exotic layout, e.g. tiling.
  // If running on Wayland assume the header bar will work (may need changing
  // if future cases occur).
  gboolean use_header_bar = TRUE;
#ifdef GDK_WINDOWING_X11
  GdkScreen* screen = gtk_window_get_screen(window);
  if (GDK_IS_X11_SCREEN(screen)) {
    const gchar* wm_name = gdk_x11_screen_get_window_manager_name(screen);
    if (g_strcmp0(wm_name, "GNOME Shell") != 0) {
      use_header_bar = FALSE;
    }
  }
#endif
  if (use_header_bar) {
    GtkHeaderBar* header_bar = GTK_HEADER_BAR(gtk_header_bar_new());
    gtk_widget_show(GTK_WIDGET(header_bar));
    gtk_header_bar_set_title(header_bar, "FluxDown");
    gtk_header_bar_set_show_close_button(header_bar, TRUE);
    gtk_window_set_titlebar(window, GTK_WIDGET(header_bar));
  } else {
    gtk_window_set_title(window, "FluxDown");
  }

  gtk_window_set_default_size(window, 1280, 720);

  // Register GResource-bundled icons into GTK's default icon theme so that
  // both X11 and Wayland compositors (including KDE Plasma) can resolve the
  // application icon by name via the standard XDG icon-theme lookup.
  gtk_icon_theme_add_resource_path(gtk_icon_theme_get_default(),
                                   "/com/fluxdown/app");
  gtk_window_set_icon_name(window, APPLICATION_ID);

  g_autoptr(FlDartProject) project = fl_dart_project_new();
  fl_dart_project_set_dart_entrypoint_arguments(
      project, self->dart_entrypoint_arguments);

  FlView* view = fl_view_new(project);
  // Keep a weak reference for use in my_application_open() — the GTK widget
  // tree owns the actual object lifetime.
  self->view = view;

  GdkRGBA background_color;
  // Background defaults to black, override it here if necessary, e.g. #00000000
  // for transparent.
  gdk_rgba_parse(&background_color, "#000000");
  fl_view_set_background_color(view, &background_color);
  gtk_widget_show(GTK_WIDGET(view));
  gtk_container_add(GTK_CONTAINER(window), GTK_WIDGET(view));

  // Show the window when Flutter renders.
  // Requires the view to be realized so we can start rendering.
  g_signal_connect_swapped(view, "first-frame", G_CALLBACK(first_frame_cb),
                           self);
  gtk_widget_realize(GTK_WIDGET(view));

  fl_register_plugins(FL_PLUGIN_REGISTRY(view));

  // Register the floating-ball MethodChannel (plan A6/S3.4). The GTK ball
  // window itself is created lazily on the first "showBall" call — it must
  // never be a GtkApplicationWindow (see floating_ball_window.h) so it can
  // outlive a hidden main window without quitting the GApplication.
  self->floating_ball =
      floating_ball_window_new(fl_engine_get_binary_messenger(fl_view_get_engine(view)));

  // 外部唤起下载小窗的原生宿主（跨端弹窗契约 v1）。同样在主引擎 messenger
  // 上注册通道；承载第二个 Flutter 引擎的弹窗窗口懒创建，首次收到 show 请
  // 求时才真正建出来。
  self->popup_host = popup_window_host_new(
      fl_engine_get_binary_messenger(fl_view_get_engine(view)));

  gtk_widget_grab_focus(GTK_WIDGET(view));
}

// Implements GApplication::open.
//
// Called when the app is asked to open files/URIs.  Two scenarios:
//
// A) First launch with files (no existing window):
//    The file URIs are already in dart_entrypoint_arguments (set by
//    local_command_line before g_application_open was called).  We just need
//    to create the window via activate(); Dart will pick up the args normally.
//
// B) Second instance forwarded to existing primary (window already running):
//    Send the file URIs to Dart via the com.fluxdown/single_instance
//    MethodChannel (same channel Windows uses via WM_COPYDATA).
static void my_application_open(GApplication* application,
                                 GFile** files,
                                 gint n_files,
                                 const gchar* /*hint*/) {
  MyApplication* self = MY_APPLICATION(application);

  GList* windows = gtk_application_get_windows(GTK_APPLICATION(application));

  if (windows != nullptr && self->view != nullptr) {
    // Scenario B: existing window — forward URIs to Dart via MethodChannel.
    g_autoptr(FlStandardMethodCodec) codec = fl_standard_method_codec_new();
    g_autoptr(FlMethodChannel) channel = fl_method_channel_new(
        fl_engine_get_binary_messenger(fl_view_get_engine(self->view)),
        "com.fluxdown/single_instance",
        FL_METHOD_CODEC(codec));

    g_autoptr(FlValue) args_list = fl_value_new_list();
    for (gint i = 0; i < n_files; i++) {
      // Prefer the URI form (file:///…) so Dart's _decodeFilePath() can handle
      // it uniformly on all platforms.
      gchar* uri = g_file_get_uri(files[i]);
      fl_value_append_take(args_list, fl_value_new_string(uri));
      g_free(uri);
    }
    fl_method_channel_invoke_method(channel, "onSecondInstance", args_list,
                                    nullptr, nullptr, nullptr);
    gtk_window_present(GTK_WINDOW(windows->data));
  } else {
    // Scenario A: first launch with files.
    // File URIs are already in dart_entrypoint_arguments; just activate.
    my_application_activate(application);
  }
}

// Implements GApplication::local_command_line.
static gboolean my_application_local_command_line(GApplication* application,
                                                  gchar*** arguments,
                                                  int* exit_status) {
  MyApplication* self = MY_APPLICATION(application);
  // Strip out the first argument as it is the binary name.
  self->dart_entrypoint_arguments = g_strdupv(*arguments + 1);

  g_autoptr(GError) error = nullptr;
  if (!g_application_register(application, nullptr, &error)) {
    g_warning("Failed to register: %s", error->message);
    *exit_status = 1;
    return TRUE;
  }

  // Collect non-option arguments as GFiles to route through
  // g_application_open().  This triggers GApplication's single-instance D-Bus
  // mechanism: on an existing primary instance our my_application_open() vfunc
  // is called with the files, enabling the MethodChannel forwarding path.
  GPtrArray* file_args =
      g_ptr_array_new_with_free_func((GDestroyNotify)g_object_unref);
  for (gchar** arg = *arguments + 1; *arg != nullptr; arg++) {
    // Skip option arguments (start with '-').
    if ((*arg)[0] == '-') continue;
    g_ptr_array_add(file_args, g_file_new_for_commandline_arg(*arg));
  }

  if (file_args->len > 0) {
    g_application_open(application, (GFile**)file_args->pdata,
                       (gint)file_args->len, "");
  } else {
    g_application_activate(application);
  }

  g_ptr_array_free(file_args, TRUE);
  *exit_status = 0;

  return TRUE;
}

// Implements GApplication::startup.
static void my_application_startup(GApplication* application) {
  // MyApplication* self = MY_APPLICATION(object);

  // Perform any actions required at application startup.

  G_APPLICATION_CLASS(my_application_parent_class)->startup(application);
}

// Implements GApplication::shutdown.
static void my_application_shutdown(GApplication* application) {
  // MyApplication* self = MY_APPLICATION(object);

  // Perform any actions required at application shutdown.

  G_APPLICATION_CLASS(my_application_parent_class)->shutdown(application);
}

// Implements GObject::dispose.
static void my_application_dispose(GObject* object) {
  MyApplication* self = MY_APPLICATION(object);
  g_clear_pointer(&self->dart_entrypoint_arguments, g_strfreev);
  g_clear_pointer(&self->floating_ball, floating_ball_window_free);
  g_clear_pointer(&self->popup_host, popup_window_host_free);
  // self->view is owned by the GTK widget tree — do not unref here.
}

static void my_application_class_init(MyApplicationClass* klass) {
  G_APPLICATION_CLASS(klass)->activate = my_application_activate;
  G_APPLICATION_CLASS(klass)->open = my_application_open;
  G_APPLICATION_CLASS(klass)->local_command_line =
      my_application_local_command_line;
  G_APPLICATION_CLASS(klass)->startup = my_application_startup;
  G_APPLICATION_CLASS(klass)->shutdown = my_application_shutdown;
  G_OBJECT_CLASS(klass)->dispose = my_application_dispose;
}

static void my_application_init(MyApplication* self) {}

MyApplication* my_application_new() {
  // Set the program name to the application ID, which helps various systems
  // like GTK and desktop environments map this running application to its
  // corresponding .desktop file. This ensures better integration by allowing
  // the application to be recognized beyond its binary name.
  g_set_prgname(APPLICATION_ID);

  return MY_APPLICATION(g_object_new(my_application_get_type(),
                                     "application-id", APPLICATION_ID, "flags",
                                     G_APPLICATION_HANDLES_OPEN, nullptr));
}
