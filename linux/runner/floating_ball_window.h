#ifndef RUNNER_FLOATING_BALL_WINDOW_H_
#define RUNNER_FLOATING_BALL_WINDOW_H_

#include <flutter_linux/flutter_linux.h>

G_BEGIN_DECLS

// Opaque native controller for the floating ball on Linux (plan A6/S3.4).
//
// Registers the com.fluxdown/floating_ball MethodChannel and answers it with
// a "dumb" GTK window: it only blits whatever bitmap Dart pushes and forwards
// pointer/drag input back over the channel. No business logic lives here.
//
// X11 only. On Wayland, queryCapability still answers (mode: "wayland") but
// showBall is a defensive no-op — GTK/X11 absolute positioning plus
// input-shape click-through (both required by the protocol) have no Wayland
// equivalent, and Dart already gates showBall behind the capability check.
//
// Deliberately NOT a GtkApplicationWindow / gtk_application_add_window(): the
// GApplication default action ("quit when the last tracked window closes")
// must never fire because the ball outlived a hidden main window. The ball
// is a plain GTK_WINDOW_TOPLEVEL the GApplication does not know about.
typedef struct _FloatingBallWindow FloatingBallWindow;

// Creates the controller and installs the method call handler on
// |messenger|. The GTK ball window is created lazily on the first
// "showBall" call.
FloatingBallWindow* floating_ball_window_new(FlBinaryMessenger* messenger);

// Tears down the GTK window (if any), removes the method call handler and
// frees |self|. Safe to call with a NULL |self|.
void floating_ball_window_free(FloatingBallWindow* self);

G_END_DECLS

#endif  // RUNNER_FLOATING_BALL_WINDOW_H_
