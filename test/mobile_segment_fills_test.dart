// Unit tests for mobileSegmentCellFills (lib/src/mobile/mobile_ui.dart).
//
// 仅测试该纯函数本身；不构造 DownloadController / SettingsProvider，不触碰
// sendSignalToRust / rustSignalStream（测试环境没有构建 libhub.so）。DownloadTask
// / SegmentData 是纯 Dart 数据类，可安全直接构造（参见 test/widget_test.dart 顶部
// 注释的约定）。

import 'package:flutter_test/flutter_test.dart';
import 'package:flux_down/src/mobile/mobile_ui.dart';
import 'package:flux_down/src/models/download_task.dart';

const _tolerance = 1e-9;

DownloadTask _task({
  required TaskStatus status,
  required int downloadedBytes,
  required int totalBytes,
  List<SegmentData>? segments,
}) {
  return DownloadTask(
    id: 't1',
    url: 'https://example.com/f.bin',
    fileName: 'f.bin',
    saveDir: 'C:/tmp',
    status: status,
    downloadedBytes: downloadedBytes,
    totalBytes: totalBytes,
    segments: segments,
  );
}

/// 不变量：返回值长度恒等于 [cells]，且每一项都落在 [0,1] 内。
void _expectWellFormed(List<double> fills, int cells) {
  expect(fills.length, cells);
  for (final v in fills) {
    expect(v, inInclusiveRange(0.0, 1.0));
  }
}

void main() {
  group('mobileSegmentCellFills', () {
    test('已完成任务即使 downloadedBytes=0 也返回全 1.0', () {
      final task = _task(
        status: TaskStatus.completed,
        downloadedBytes: 0,
        totalBytes: 1000,
      );
      final fills = mobileSegmentCellFills(task, 6);

      _expectWellFormed(fills, 6);
      for (final v in fills) {
        expect(v, closeTo(1.0, _tolerance));
      }
    });

    test('totalBytes<=0 且下载中 → 返回全 0.0', () {
      final task = _task(
        status: TaskStatus.downloading,
        downloadedBytes: 0,
        totalBytes: 0,
      );
      final fills = mobileSegmentCellFills(task, 6);

      _expectWellFormed(fills, 6);
      for (final v in fills) {
        expect(v, closeTo(0.0, _tolerance));
      }
    });

    test('无 segments 时按整体进度做前缀填充，含跨格边界的部分填充格', () {
      // total=1000, cells=10 → 每格 100 字节；downloaded=453 → progress=0.453
      // （未触及 0.999 上限），filled = progress*cells = 4.53：
      // 第 i 格 = clamp(4.53 - i, 0, 1) → 前 4 格（index 0-3）落满 1.0，
      // 第 5 格（index 4）部分填充 = 0.53（真正落在 (0,1) 区间，用于验证
      // 分数边界处的 clamp 行为），其余格为 0。
      final task = _task(
        status: TaskStatus.downloading,
        downloadedBytes: 453,
        totalBytes: 1000,
      );
      final fills = mobileSegmentCellFills(task, 10);

      _expectWellFormed(fills, 10);
      for (var i = 0; i < 4; i++) {
        expect(fills[i], closeTo(1.0, _tolerance));
      }
      expect(fills[4], closeTo(0.53, 1e-9));
      expect(fills[4], greaterThan(0.0));
      expect(fills[4], lessThan(1.0));
      for (var i = 5; i < 10; i++) {
        expect(fills[i], closeTo(0.0, _tolerance));
      }
    });

    test('单个 segment 恰好覆盖前半区间 → 前半格 1.0，后半格 0.0', () {
      final task = _task(
        status: TaskStatus.downloading,
        downloadedBytes: 480,
        totalBytes: 960,
        segments: const [
          SegmentData(
            index: 0,
            startByte: 0,
            endByte: 959,
            downloadedBytes: 480,
          ),
        ],
      );
      final fills = mobileSegmentCellFills(task, 48);

      _expectWellFormed(fills, 48);
      for (var i = 0; i < 24; i++) {
        expect(fills[i], closeTo(1.0, _tolerance));
      }
      for (var i = 24; i < 48; i++) {
        expect(fills[i], closeTo(0.0, _tolerance));
      }
    });

    test('segment 跨格边界时按字节重叠比例分摊到相邻两格', () {
      // total=100, cells=10 → 每格 10 字节；已下载区间 [5,15) 横跨格 0
      // ([0,10)) 与格 1 ([10,20))，各自贡献一半字节。
      final task = _task(
        status: TaskStatus.downloading,
        downloadedBytes: 10,
        totalBytes: 100,
        segments: const [
          SegmentData(
            index: 0,
            startByte: 5,
            endByte: 99,
            downloadedBytes: 10,
          ),
        ],
      );
      final fills = mobileSegmentCellFills(task, 10);

      _expectWellFormed(fills, 10);
      expect(fills[0], closeTo(0.5, _tolerance));
      expect(fills[1], closeTo(0.5, _tolerance));
      for (var i = 2; i < 10; i++) {
        expect(fills[i], closeTo(0.0, _tolerance));
      }
    });

    test('多个互不重叠的 segment 分别映射到各自对应的格子', () {
      final task = _task(
        status: TaskStatus.downloading,
        downloadedBytes: 30,
        totalBytes: 100,
        segments: const [
          SegmentData(
            index: 0,
            startByte: 0,
            endByte: 9,
            downloadedBytes: 10,
          ),
          SegmentData(
            index: 1,
            startByte: 30,
            endByte: 39,
            downloadedBytes: 10,
          ),
          SegmentData(
            index: 2,
            startByte: 70,
            endByte: 79,
            downloadedBytes: 10,
          ),
        ],
      );
      final fills = mobileSegmentCellFills(task, 10);

      _expectWellFormed(fills, 10);
      expect(fills[0], closeTo(1.0, _tolerance));
      expect(fills[3], closeTo(1.0, _tolerance));
      expect(fills[7], closeTo(1.0, _tolerance));
      for (final i in [1, 2, 4, 5, 6, 8, 9]) {
        expect(fills[i], closeTo(0.0, _tolerance));
      }
    });

    test('downloadedBytes<=0 的 segment 被跳过 → 结果全 0', () {
      final task = _task(
        status: TaskStatus.downloading,
        downloadedBytes: 0,
        totalBytes: 100,
        segments: const [
          SegmentData(
            index: 0,
            startByte: 0,
            endByte: 9,
            downloadedBytes: 0,
          ),
        ],
      );
      final fills = mobileSegmentCellFills(task, 10);

      _expectWellFormed(fills, 10);
      for (final v in fills) {
        expect(v, closeTo(0.0, _tolerance));
      }
    });
  });
}
