/// 快速下载表单结果的统一提交器（主 isolate）。
///
/// 主窗口对话框与外部唤起独立小窗共用：解析多行 URL、记录用户偏好
/// （上次保存目录 / 上次线程数 / 上次目标设备）、按单条/批量分别发送
/// [ConfirmExternalDownload] / [BatchCreateTask] 信号；选了目标设备时改为
/// 下发（本地配对设备走 [LocalPairingService.dispatchTask]，云账户设备走
/// [CloudClient.dispatchTask]），成功/失败经 [initQuickDownloadSubmitter]
/// 注入的主窗口 toast 通道反馈——独立小窗提交后即刻关闭，下发完成时
/// 多半已经没有活着的表单 context 可用。
library;

import 'dart:async';

import 'package:flutter/widgets.dart';
import 'package:shadcn_ui/shadcn_ui.dart';

import '../bindings/bindings.dart';
import '../i18n/locale_provider.dart';
import '../models/download_queue.dart';
import '../models/settings_provider.dart';
import '../widgets/flux_sonner.dart';
import '../widgets/quick_download_form.dart';
import 'cloud/cloud_auth_service.dart';
import 'cloud/cloud_client.dart';
import 'link/link_models.dart';
import 'link/local_pairing_service.dart';
import 'log_service.dart';

const _tag = 'QuickSubmit';

// 主引擎侧 root navigator —— toast 反馈通道（下发是异步的，独立小窗场景
// 下完成时弹窗多半已经关闭，结果提示必须走主窗口的 FluxSonner；由
// main.dart 启动时注入一次，接线时机同 HlsQualityService 等服务）。
GlobalKey<NavigatorState>? _navigatorKey;

/// 供 main.dart 启动时调用一次。
void initQuickDownloadSubmitter({
  required GlobalKey<NavigatorState> navigatorKey,
}) {
  _navigatorKey = navigatorKey;
}

void _showResultToast({required bool success, required String deviceName}) {
  final context = _navigatorKey?.currentContext;
  if (context == null || !context.mounted) return;
  if (success) {
    FluxSonner.of(
      context,
    ).show(ShadToast(title: Text(currentS.dispatchedToDevice(deviceName))));
  } else {
    FluxSonner.of(
      context,
    ).show(ShadToast.destructive(title: Text(currentS.dispatchFailed)));
  }
}

/// 解析表单结果并发送下载信号。
///
/// [referrer] / [hintFileSize] 为外部请求上下文（保存在主引擎侧，
/// 不经过表单流转）；cookies 由表单携带（预填浏览器捕获值，用户可编辑）。
void submitQuickDownload({
  required QuickDownloadFormResult result,
  required String referrer,
  required int hintFileSize,
  String audioUrlOverride = '',
}) {
  final saveDir = result.saveDir.trim();
  if (saveDir.isEmpty) {
    logError(_tag, 'empty save dir, dropping submit');
    return;
  }
  final entries = parseQuickDownloadEntries(result.urlText);
  if (entries.isEmpty) {
    logError(_tag, 'no valid entries, dropping submit');
    return;
  }

  // 稍后下载且未选队列 → 落入内置「稍后下载」队列，等「启动队列」批量恢复；
  // 已显式选择队列则尊重选择（暂停加入该队列）。
  final queueId = (result.startLater && result.queueId.isEmpty)
      ? kLaterQueueId
      : result.queueId;

  // 记录本次保存位置，供"跟随上次保存位置"开关使用
  SettingsProvider.globalInstance?.recordLastSaveDir(saveDir);

  // 记住用户本次选择的线程数，下次新建时沿用
  if (result.threadsUserModified) {
    SettingsProvider.globalInstance?.setLastDialogThreads(
      result.segments > 0 ? result.segments.toString() : 'auto',
    );
  }

  final targetDeviceId = result.targetDeviceId.trim();
  if (targetDeviceId.isNotEmpty) {
    // 目标设备可能是云账户设备，也可能是本地配对设备指纹——两者复用同一
    // 字段（表单侧不区分来源，见 QuickDownloadFormResult.targetDeviceId
    // 文档），按指纹是否命中本地名册分流。判定只能在主引擎侧做：本函数
    // 恒跑在主 isolate，LocalPairingService 此时已 attach；独立小窗
    // isolate 没有这份名册，选择器只是把指纹原样带回来（见
    // quick_download_form.dart 的 _localDeviceOptions）。
    SettingsProvider.globalInstance?.setLastTargetDevice(targetDeviceId);
    final localDevice = LocalPairingService.instance.localDevices
        .where((d) => d.fingerprint == targetDeviceId)
        .firstOrNull;
    if (localDevice != null) {
      unawaited(
        _dispatchToLocalDevice(
          localDevice,
          entries,
          saveDir,
          result.rename.trim(),
        ),
      );
      return;
    }

    // 云账户设备 — 全部走 Dart 层调云 API，不改本机 rinf bincode 信号路径
    // （本机路径见下方 entries.length==1/else 分支，完全不变）。
    final deviceName =
        CloudAuthService.instance.remoteDevices
            .where((d) => d.deviceId == targetDeviceId)
            .firstOrNull
            ?.name ??
        targetDeviceId;
    CloudClient.instance
        .dispatchTask(
          toDevice: targetDeviceId,
          url: result.urlText,
          saveDir: saveDir,
        )
        .then((task) {
          logInfo(_tag, 'dispatched task ${task.id} to device=$targetDeviceId');
          _showResultToast(success: true, deviceName: deviceName);
        })
        .catchError((Object e) {
          logError(_tag, 'dispatch to device=$targetDeviceId failed: $e');
          _showResultToast(success: false, deviceName: deviceName);
        });
    return;
  }

  if (entries.length == 1) {
    final entry = entries.first;
    final fileName = result.rename.isNotEmpty ? result.rename : entry.fileName;
    // audioUrl 走独立通道透传（外部 track-pair 请求，不在 URL 文本里），
    // 优先 result（表单透传的原始值），回退文本解析出的 checksum= 轨对。
    final audioUrl = audioUrlOverride.isNotEmpty
        ? audioUrlOverride
        : (result.audioUrl.isNotEmpty ? result.audioUrl : entry.audioUrl);
    // 校验值：高级选项手填的优先，否则回退 URL 文本里的 checksum= 选项行
    final checksum = result.checksum.isNotEmpty
        ? result.checksum
        : entry.checksum;
    if (checksum.isEmpty) {
      // 单条无校验 — 使用 ConfirmExternalDownload（保留 Rust 侧按 URL
      // 缓存的请求上下文与 hintFileSize，免二次探测）
      ConfirmExternalDownload(
        url: entry.url,
        saveDir: saveDir,
        fileName: fileName,
        segments: result.segments,
        cookies: result.cookies,
        referrer: referrer,
        hintFileSize: hintFileSize,
        proxyUrl: result.proxyUrl,
        userAgent: result.userAgent,
        queueId: queueId,
        ignoreTlsErrors: result.ignoreTlsErrors,
        startPaused: result.startLater,
        audioUrl: audioUrl,
        extraHeaders: result.extraHeaders,
        httpUser: result.httpUser,
        httpPassword: result.httpPassword,
        saveSiteAuth: result.saveSiteAuth,
        // 用户在场确认路径：保留二次选择弹窗
        unattended: false,
      ).sendSignalToRust();
    } else {
      // 单条带校验 — ConfirmExternalDownload 信号无 checksum 字段，
      // 走 BatchCreateTask 单元素路径（UrlEntry 携带 checksum；
      // 代价仅是丢失 hintFileSize，由元数据探测补齐）
      BatchCreateTask(
        entries: [
          UrlEntry(
            url: entry.url,
            fileName: fileName,
            checksum: checksum,
            audioUrl: audioUrl,
          ),
        ],
        saveDir: saveDir,
        segments: result.segments,
        proxyUrl: result.proxyUrl,
        userAgent: result.userAgent,
        queueId: queueId,
        ignoreTlsErrors: result.ignoreTlsErrors,
        startPaused: result.startLater,
        cookies: result.cookies,
        referrer: referrer,
        extraHeaders: result.extraHeaders,
        // 用户在场确认路径：保留二次选择弹窗
        unattended: false,
      ).sendSignalToRust();
    }
  } else {
    // 多条 — 使用 BatchCreateTask（携带每条的 fileName/checksum）
    BatchCreateTask(
      entries: entries
          .map(
            (e) => UrlEntry(
              url: e.url,
              fileName: e.fileName,
              checksum: e.checksum,
              audioUrl: e.audioUrl,
            ),
          )
          .toList(),
      saveDir: saveDir,
      segments: result.segments,
      proxyUrl: result.proxyUrl,
      userAgent: result.userAgent,
      queueId: queueId,
      ignoreTlsErrors: result.ignoreTlsErrors,
      startPaused: result.startLater,
      cookies: result.cookies,
      referrer: referrer,
      extraHeaders: result.extraHeaders,
      // 用户在场确认路径：保留二次选择弹窗
      unattended: false,
    ).sendSignalToRust();
  }
}

/// 下发给本地配对设备（局域网直连，走 Rust 端 LinkManager，不经云
/// API）。多条批量逐条下发，任一条失败即中止（与主窗口新建下载对话框的
/// 云端下发行为一致）；成功/失败经 [_showResultToast] 反馈——下发完成时
/// 独立小窗多半已经关闭，不能假设有一个存活的表单 context。
Future<void> _dispatchToLocalDevice(
  LocalDevice device,
  List<QuickDownloadEntry> entries,
  String saveDir,
  String rename,
) async {
  final svc = LocalPairingService.instance;
  try {
    for (final entry in entries) {
      final fileName = entries.length == 1 && rename.isNotEmpty
          ? rename
          : entry.fileName;
      await svc.dispatchTask(
        fingerprint: device.fingerprint,
        url: entry.url,
        saveDir: saveDir,
        fileName: fileName,
      );
    }
    logInfo(_tag, 'dispatched to local device=${device.fingerprint}');
    _showResultToast(success: true, deviceName: device.name);
  } catch (e) {
    logError(
      _tag,
      'dispatch to local device=${device.fingerprint} failed: $e',
    );
    _showResultToast(success: false, deviceName: device.name);
  }
}
