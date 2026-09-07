// 未确认文件名的占位展示契约：用任务 URL 顶替「未知文件」（对齐 Web SPA
// 的 fileName || url），超长 URL 截断为 64 字符加省略号。
import 'package:flutter_test/flutter_test.dart';
import 'package:flux_down/src/bindings/bindings.dart';
import 'package:flux_down/src/models/download_task.dart';

void main() {
  TaskInfo makeInfo({required String url, required String fileName}) =>
      TaskInfo(
        taskId: 't1',
        url: url,
        fileName: fileName,
        saveDir: '/tmp',
        status: 0,
        downloadedBytes: 0,
        totalBytes: 0,
        errorMessage: '',
        createdAt: '1700000000',
        proxyUrl: '',
        queueId: '',
        checksum: '',
        ignoreTlsErrors: false,
        fileMissing: false,
        completedAt: '',
        segments: 0,
        queueOrder: 0,
        uploadedBytes: 0,
        uploadedAtCompletion: 0,
        seedingStatus: 0,
        seedingMessage: '',
        seedingTimeSecs: 0,
        seedRatioLimitMilli: -2,
        seedPostRatioLimitMilli: -2,
        seedTimeLimitMinutes: -2,
        seedInactiveTimeLimitMinutes: -2,
        seedUploadLimitBps: 0,
        referrer: '',
        groupId: '',
        rssSourceId: '',
        originUrl: '',
        autoRoute: '',
      );

  test('short URL used verbatim as placeholder', () {
    const url = 'https://example.com/dl?id=42';
    expect(placeholderTaskName(url), url);
  });

  test('long URL truncated to 64 chars plus ellipsis', () {
    final url = 'magnet:?xt=urn:btih:${'a' * 100}';
    final name = placeholderTaskName(url);
    expect(name.length, 65);
    expect(name, '${url.substring(0, 64)}…');
    expect(name.endsWith('…'), isTrue);
  });

  test('fromTaskInfo without name falls back to URL placeholder', () {
    final task = DownloadTask.fromTaskInfo(
      makeInfo(url: 'https://example.com/dl?id=42', fileName: ''),
    );
    expect(task.fileName, 'https://example.com/dl?id=42');
    expect(task.fileNameConfirmed, isFalse);
  });

  test('fromTaskInfo with confirmed name ignores placeholder', () {
    final task = DownloadTask.fromTaskInfo(
      makeInfo(url: 'https://example.com/dl?id=42', fileName: 'f.zip'),
    );
    expect(task.fileName, 'f.zip');
    expect(task.fileNameConfirmed, isTrue);
  });
}
