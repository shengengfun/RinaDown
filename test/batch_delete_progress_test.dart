// Tests for batch delete progress state machine in DownloadController.
//
// DownloadController cannot be instantiated in unit tests because its constructor
// immediately subscribes to rinf Rust signal streams (which require native code).
// These tests therefore replicate the exact state-machine logic from three
// code paths, using the same variable names and formulas:
//
//   deleteCheckedTasks  → download_controller.dart:404-411
//   _onProgress handler → download_controller.dart:857-880
//   batchDeleteProgress → download_controller.dart:54-55

import 'package:flutter_test/flutter_test.dart';

void main() {
  const progressThreshold = 20;

  // ---------------------------------------------------------------------------
  // Helpers that mirror the (fixed) state machine in DownloadController.
  // ---------------------------------------------------------------------------

  /// Mirrors the fixed deleteCheckedTasks block (download_controller.dart:404-411).
  void simulateDeleteCheckedTasks(
    List<String> ids,
    Set<String> pendingDeleteIds,
    int Function() getBatchDeleteDone,
    void Function(int) setBatchDeleteTotal,
  ) {
    if (ids.length >= progressThreshold || pendingDeleteIds.isNotEmpty) {
      pendingDeleteIds.addAll(ids);
      // total = already confirmed + all still-pending (fixed formula)
      setBatchDeleteTotal(getBatchDeleteDone() + pendingDeleteIds.length);
    }
  }

  /// Mirrors one Rust confirmation arriving via _onProgress.
  void simulateRustConfirmation(
    String taskId,
    Set<String> pendingDeleteIds,
    int Function() getBatchDeleteDone,
    void Function(int) setBatchDeleteDone,
  ) {
    if (pendingDeleteIds.contains(taskId)) {
      pendingDeleteIds.remove(taskId);
      setBatchDeleteDone(getBatchDeleteDone() + 1);
    }
  }

  // ---------------------------------------------------------------------------

  group('batchDeleteProgress — single batch', () {
    test('progress stays within [0.0, 1.0] throughout', () {
      final pendingDeleteIds = <String>{};
      var batchDeleteDone = 0;
      var batchDeleteTotal = 0;

      double progress() =>
          batchDeleteTotal > 0 ? batchDeleteDone / batchDeleteTotal : 0.0;

      final ids = List.generate(25, (i) => 'task-$i');
      simulateDeleteCheckedTasks(
        ids,
        pendingDeleteIds,
        () => batchDeleteDone,
        (v) => batchDeleteTotal = v,
      );

      expect(batchDeleteTotal, 25);
      expect(pendingDeleteIds.length, 25);

      for (final id in ids) {
        simulateRustConfirmation(
          id,
          pendingDeleteIds,
          () => batchDeleteDone,
          (v) => batchDeleteDone = v,
        );
        expect(progress(), lessThanOrEqualTo(1.0),
            reason: 'done=$batchDeleteDone total=$batchDeleteTotal');
      }

      expect(progress(), 1.0);
      expect(pendingDeleteIds, isEmpty);
    });

    test('large batch (10 000 tasks): total and done are consistent', () {
      final pendingDeleteIds = <String>{};
      var batchDeleteDone = 0;
      var batchDeleteTotal = 0;

      double progress() =>
          batchDeleteTotal > 0 ? batchDeleteDone / batchDeleteTotal : 0.0;

      const n = 10000;
      final ids = List.generate(n, (i) => 'task-$i');
      simulateDeleteCheckedTasks(
        ids,
        pendingDeleteIds,
        () => batchDeleteDone,
        (v) => batchDeleteTotal = v,
      );

      expect(batchDeleteTotal, n);

      for (final id in ids) {
        simulateRustConfirmation(
          id,
          pendingDeleteIds,
          () => batchDeleteDone,
          (v) => batchDeleteDone = v,
        );
      }

      expect(batchDeleteDone, n);
      expect(progress(), 1.0);
      expect(pendingDeleteIds, isEmpty);
    });
  });

  group('batchDeleteProgress — overlapping batches (previously broken)', () {
    test(
      'second large batch during first: total accounts for all pending IDs, '
      'progress never exceeds 1.0',
      () {
        final pendingDeleteIds = <String>{};
        var batchDeleteDone = 0;
        var batchDeleteTotal = 0;

        double progress() =>
            batchDeleteTotal > 0 ? batchDeleteDone / batchDeleteTotal : 0.0;

        // ── First batch: 25 tasks ────────────────────────────────────────────
        final batch1 = List.generate(25, (i) => 'task1-$i');
        simulateDeleteCheckedTasks(
          batch1,
          pendingDeleteIds,
          () => batchDeleteDone,
          (v) => batchDeleteTotal = v,
        );
        expect(batchDeleteTotal, 25);

        // 10 of 25 confirm.
        for (var i = 0; i < 10; i++) {
          simulateRustConfirmation(
            'task1-$i',
            pendingDeleteIds,
            () => batchDeleteDone,
            (v) => batchDeleteDone = v,
          );
        }
        expect(batchDeleteDone, 10);
        expect(pendingDeleteIds.length, 15);

        // ── Second batch: 30 tasks, initiated while first is in-progress ─────
        final batch2 = List.generate(30, (i) => 'task2-$i');
        simulateDeleteCheckedTasks(
          batch2,
          pendingDeleteIds,
          () => batchDeleteDone,
          (v) => batchDeleteTotal = v,
        );

        // total must reflect ALL pending (15 leftover + 30 new) + already done.
        expect(pendingDeleteIds.length, 45,
            reason: 'both batches must be tracked');
        expect(batchDeleteTotal, 10 + 45,
            reason: 'total = confirmed so far + all pending');
        expect(batchDeleteDone, 10, reason: 'confirmed count must not reset');

        // All 45 remaining IDs confirm.
        final remaining = [
          ...List.generate(15, (i) => 'task1-${i + 10}'),
          ...batch2,
        ];
        for (final id in remaining) {
          simulateRustConfirmation(
            id,
            pendingDeleteIds,
            () => batchDeleteDone,
            (v) => batchDeleteDone = v,
          );
          expect(progress(), lessThanOrEqualTo(1.0),
              reason:
                  'progress must stay ≤ 1.0 (done=$batchDeleteDone total=$batchDeleteTotal)');
        }

        expect(batchDeleteDone, 55);
        expect(batchDeleteTotal, 55);
        expect(progress(), 1.0);
        expect(pendingDeleteIds, isEmpty);
      },
    );

    test(
      'sub-threshold second batch while first in-progress: gets merged into tracking',
      () {
        final pendingDeleteIds = <String>{};
        var batchDeleteDone = 0;
        var batchDeleteTotal = 0;

        double progress() =>
            batchDeleteTotal > 0 ? batchDeleteDone / batchDeleteTotal : 0.0;

        // First batch: 25 tasks (>= threshold).
        final batch1 = List.generate(25, (i) => 'task1-$i');
        simulateDeleteCheckedTasks(
          batch1,
          pendingDeleteIds,
          () => batchDeleteDone,
          (v) => batchDeleteTotal = v,
        );

        // 10 confirm.
        for (var i = 0; i < 10; i++) {
          simulateRustConfirmation(
            'task1-$i',
            pendingDeleteIds,
            () => batchDeleteDone,
            (v) => batchDeleteDone = v,
          );
        }

        // Second batch: only 5 tasks (< threshold=20).
        // Previously these were silently ignored; now they are merged because
        // pendingDeleteIds.isNotEmpty triggers the tracking condition.
        final batch2 = List.generate(5, (i) => 'task2-$i');
        simulateDeleteCheckedTasks(
          batch2,
          pendingDeleteIds,
          () => batchDeleteDone,
          (v) => batchDeleteTotal = v,
        );

        // Both batches are now tracked.
        expect(pendingDeleteIds.length, 20,
            reason: '15 remaining from batch1 + 5 from batch2');
        expect(batchDeleteTotal, 10 + 20,
            reason: 'total = 10 confirmed + 20 pending');

        // All 20 remaining confirm.
        for (final id in [
          ...List.generate(15, (i) => 'task1-${i + 10}'),
          ...batch2,
        ]) {
          simulateRustConfirmation(
            id,
            pendingDeleteIds,
            () => batchDeleteDone,
            (v) => batchDeleteDone = v,
          );
          expect(progress(), lessThanOrEqualTo(1.0));
        }

        expect(progress(), 1.0);
        expect(pendingDeleteIds, isEmpty);
      },
    );

    test(
      'large overlapping batches (10 000 + 10 000): progress is monotone and '
      'never exceeds 1.0',
      () {
        final pendingDeleteIds = <String>{};
        var batchDeleteDone = 0;
        var batchDeleteTotal = 0;

        double progress() =>
            batchDeleteTotal > 0 ? batchDeleteDone / batchDeleteTotal : 0.0;

        const n = 10000;

        // First batch.
        final batch1 = List.generate(n, (i) => 'a-$i');
        simulateDeleteCheckedTasks(
          batch1,
          pendingDeleteIds,
          () => batchDeleteDone,
          (v) => batchDeleteTotal = v,
        );

        // Half of batch1 confirms.
        for (var i = 0; i < n ~/ 2; i++) {
          simulateRustConfirmation(
            'a-$i',
            pendingDeleteIds,
            () => batchDeleteDone,
            (v) => batchDeleteDone = v,
          );
        }
        expect(batchDeleteDone, n ~/ 2);

        // Second batch of the same size.
        final batch2 = List.generate(n, (i) => 'b-$i');
        simulateDeleteCheckedTasks(
          batch2,
          pendingDeleteIds,
          () => batchDeleteDone,
          (v) => batchDeleteTotal = v,
        );

        final expectedTotal = (n ~/ 2) + (n ~/ 2) + n; // 5000 + 5000 + 10000
        expect(batchDeleteTotal, expectedTotal);

        var prevProgress = progress();

        // All remaining confirm.
        for (final id in [
          ...List.generate(n ~/ 2, (i) => 'a-${i + n ~/ 2}'),
          ...batch2,
        ]) {
          simulateRustConfirmation(
            id,
            pendingDeleteIds,
            () => batchDeleteDone,
            (v) => batchDeleteDone = v,
          );
          final p = progress();
          expect(p, lessThanOrEqualTo(1.0),
              reason: 'progress must not exceed 1.0');
          expect(p, greaterThanOrEqualTo(prevProgress),
              reason: 'progress must be monotonically non-decreasing');
          prevProgress = p;
        }

        expect(progress(), 1.0);
        expect(pendingDeleteIds, isEmpty);
      },
    );
  });

  group('batchDeleteProgress — notification throttle', () {
    // Verify the throttle formula: max ~100 notifies per batch regardless of size.
    test('step = ceil(total / 100) clamped to [1, total]', () {
      // small batch: step=1 (every confirmation triggers notify)
      expect((25 / 100).ceil().clamp(1, 25), 1);
      // medium batch
      expect((500 / 100).ceil().clamp(1, 500), 5);
      // large batch: step=100 (at most 100 notifies for 10 000 tasks)
      expect((10000 / 100).ceil().clamp(1, 10000), 100);
      // very large batch: step=1000
      expect((100000 / 100).ceil().clamp(1, 100000), 1000);
    });

    test('number of notify calls is bounded by ~100 for 10 000 tasks', () {
      const n = 10000;
      final pendingDeleteIds = <String>{};
      var batchDeleteDone = 0;
      var batchDeleteTotal = 0;
      var notifyCount = 0;

      final ids = List.generate(n, (i) => 'task-$i');
      simulateDeleteCheckedTasks(
        ids,
        pendingDeleteIds,
        () => batchDeleteDone,
        (v) => batchDeleteTotal = v,
      );

      for (final id in ids) {
        simulateRustConfirmation(
          id,
          pendingDeleteIds,
          () => batchDeleteDone,
          (v) => batchDeleteDone = v,
        );
        // Mirror the throttle logic from _onProgress.
        final isDone = pendingDeleteIds.isEmpty;
        final step =
            (batchDeleteTotal / 100).ceil().clamp(1, batchDeleteTotal);
        if (isDone || batchDeleteDone % step == 0) {
          notifyCount++;
        }
      }

      // Allow up to 101 notifies (100 periodic + 1 forced on completion).
      expect(notifyCount, lessThanOrEqualTo(101),
          reason: 'at most ~100 UI rebuilds for $n deletions');
      // Must notify at least once (the completion event).
      expect(notifyCount, greaterThanOrEqualTo(1));
    });
  });
}
