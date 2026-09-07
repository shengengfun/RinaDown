// Tests for the pure Shift 范围选择区间计算函数.
//
// Source: lib/src/models/task_selection.dart (taskSelectionRange). Pure
// function, no DownloadController instantiation needed (it requires rinf
// FFI, see scout-dart.md §13).

import 'package:flutter_test/flutter_test.dart';
import 'package:flux_down/src/models/task_selection.dart';

void main() {
  group('taskSelectionRange', () {
    const ids = ['a', 'b', 'c', 'd', 'e'];

    test('anchor 在前：正序返回闭区间', () {
      expect(taskSelectionRange(ids, 'b', 'd'), ['b', 'c', 'd']);
    });

    test('anchor 在后：逆序输入，结果仍按 orderedIds 的顺序排列', () {
      expect(taskSelectionRange(ids, 'd', 'b'), ['b', 'c', 'd']);
    });

    test('anchor 不在 orderedIds 中：退化为只选中 target', () {
      expect(taskSelectionRange(ids, 'zzz', 'c'), ['c']);
    });

    test('target 不在 orderedIds 中：返回空列表', () {
      expect(taskSelectionRange(ids, 'b', 'zzz'), <String>[]);
    });

    test('anchor 与 target 都不在 orderedIds 中：返回空列表', () {
      expect(taskSelectionRange(ids, 'zzz', 'yyy'), <String>[]);
    });

    test('anchor 与 target 相同：返回单元素列表', () {
      expect(taskSelectionRange(ids, 'c', 'c'), ['c']);
    });

    test('相邻元素：区间恰为两元素', () {
      expect(taskSelectionRange(ids, 'a', 'b'), ['a', 'b']);
    });

    test('orderedIds 为空：target 必不存在，返回空列表', () {
      expect(taskSelectionRange(const [], 'a', 'b'), <String>[]);
    });
  });

  group('taskSelectionRangeForTap', () {
    const ids = ['a', 'b', 'c', 'd'];

    test('尚未进入管理模式时以普通点击选中的任务为 Shift 锚点', () {
      expect(
        taskSelectionRangeForTap(
          ids,
          rangeAnchorId: null,
          selectedTaskId: 'a',
          targetId: 'c',
        ),
        ['a', 'b', 'c'],
      );
    });

    test('已有管理模式锚点时优先使用该锚点', () {
      expect(
        taskSelectionRangeForTap(
          ids,
          rangeAnchorId: 'b',
          selectedTaskId: 'a',
          targetId: 'd',
        ),
        ['b', 'c', 'd'],
      );
    });

    test('普通选中任务不在当前可见顺序时退化为目标任务', () {
      expect(
        taskSelectionRangeForTap(
          ids,
          rangeAnchorId: null,
          selectedTaskId: 'hidden',
          targetId: 'c',
        ),
        ['c'],
      );
    });
  });
}
