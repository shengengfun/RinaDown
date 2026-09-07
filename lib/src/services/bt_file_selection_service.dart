import 'dart:async';

import 'package:flutter/scheduler.dart';
import 'package:flutter/widgets.dart';
import 'package:rinf/rinf.dart';

import '../bindings/bindings.dart';
import '../services/log_service.dart';
import '../widgets/bt_file_selection_dialog.dart';

const _tag = 'BtFileSelectionSvc';

/// Service that receives [BtFilesInfo] signals from Rust and either:
///   1. Routes the signal to a registered in-dialog callback (when the
///      new-download dialog is waiting for a magnet-link file list), or
///   2. Shows the standalone [BtFileSelectionDialog] (legacy path for
///      tasks that were created outside the new-download dialog).
///
/// The callback-registration approach is race-free: the new-download
/// dialog registers its handler *before* submitting the CreateTask signal,
/// so by the time Rust resolves the magnet metadata and sends BtFilesInfo,
/// the callback is already installed — no taskId matching required.
class BtFileSelectionService {
  static BtFileSelectionService? _instance;

  final GlobalKey<NavigatorState> navigatorKey;
  StreamSubscription<RustSignalPack<BtFilesInfo>>? _sub;

  /// Tasks that are currently showing a standalone dialog, keyed by taskId.
  final Set<String> _openDialogTaskIds = {};

  /// If non-null, the *next* BtFilesInfo signal will be handed to this
  /// callback instead of opening a standalone dialog.  The new-download
  /// dialog registers this handler immediately before calling createTask(),
  /// so there is no window where the signal could arrive unhandled.
  ///
  /// After the callback is invoked once it is cleared automatically, so
  /// subsequent signals (e.g. if the user pauses and resumes the same task)
  /// fall back to the standalone dialog path.
  static void Function(BtFilesInfo)? _pendingDialogHandler;

  BtFileSelectionService._({required this.navigatorKey});

  // ─── Lifecycle ────────────────────────────────────────────────────────────

  static void init({required GlobalKey<NavigatorState> navigatorKey}) {
    logInfo(_tag, 'init');
    _instance?._teardown();
    _instance = BtFileSelectionService._(navigatorKey: navigatorKey);
    _instance!._startListening();
  }

  static void shutdown() {
    logInfo(_tag, 'shutdown');
    _instance?._teardown();
    _instance = null;
    _pendingDialogHandler = null;
  }

  void _teardown() {
    _sub?.cancel();
    _sub = null;
    _openDialogTaskIds.clear();
  }

  void _startListening() {
    _sub = BtFilesInfo.rustSignalStream.listen(_onFilesInfo);
  }

  // ─── Callback registration (used by new-download dialog) ─────────────────

  /// Register a one-shot handler for the next [BtFilesInfo] signal.
  ///
  /// Call this immediately *before* submitting a CreateTask signal so that
  /// the handler is in place before Rust can possibly send BtFilesInfo.
  /// The handler is invoked at most once and then cleared automatically.
  ///
  /// Pass `null` to cancel a previously registered handler (e.g. when the
  /// dialog is closed before the signal arrives).
  static void registerPendingHandler(void Function(BtFilesInfo)? handler) {
    logInfo(_tag, handler != null ? 'registerPendingHandler: set' : 'registerPendingHandler: cleared');
    _pendingDialogHandler = handler;
  }

  // ─── Signal handling ──────────────────────────────────────────────────────

  void _onFilesInfo(RustSignalPack<BtFilesInfo> pack) {
    final msg = pack.message;
    logInfo(
      _tag,
      'received BtFilesInfo: task=${msg.taskId}, files=${msg.files.length}',
    );

    // Path 1: a new-download dialog is waiting for this signal.
    // Hand it directly to the registered callback and clear the slot.
    final handler = _pendingDialogHandler;
    if (handler != null) {
      logInfo(_tag, 'routing BtFilesInfo to in-dialog handler');
      _pendingDialogHandler = null;
      handler(msg);
      return;
    }

    // Path 2: no dialog handler registered — show the standalone dialog.

    // Deduplicate: if a dialog is already open for this exact task, ignore.
    if (_openDialogTaskIds.contains(msg.taskId)) {
      logInfo(_tag, 'standalone dialog already open for task=${msg.taskId}, ignoring');
      return;
    }

    final context = navigatorKey.currentContext;
    if (context == null || !context.mounted) {
      logInfo(_tag, 'no valid context — auto-selecting all for task=${msg.taskId}');
      _autoSelectAll(msg);
      return;
    }

    _openDialogTaskIds.add(msg.taskId);
    showBtFileSelectionDialog(
      context,
      taskId: msg.taskId,
      totalBytes: msg.totalBytes.toInt(),
      files: msg.files,
      onClosed: () {
        _openDialogTaskIds.remove(msg.taskId);
        logInfo(_tag, 'standalone dialog closed for task=${msg.taskId}');
      },
    );
  }

  /// Fallback: automatically select all files when no UI context is available.
  void _autoSelectAll(BtFilesInfo msg) {
    void send() {
      logInfo(_tag, 'auto-selecting all ${msg.files.length} file(s) for task=${msg.taskId}');
      SelectBtFiles(
        taskId: msg.taskId,
        selectedIndices: msg.files.map((f) => f.index).toList(),
      ).sendSignalToRust();
    }
    SchedulerBinding.instance.addPostFrameCallback((_) => send());
  }
}
