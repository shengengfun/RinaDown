// 入站配对核验弹窗 —— 本机作为「被添加方」收到配对请求时展示的核验 UI。
//
// 背景：配对此前是单边确认——只有发起方能看到 SAS 短码并决定是否配对，被
// 添加的这台设备全程无感知、无否决权；协议文档声称的「SAS 双端肉眼核对防
// 中间人」实际只有单边生效。后端已改为双端核验：本机收到配对 hello 时会
// 推 `LinkEvent{kind:"incomingPairing"}`，并在对端发来 confirm 请求后阻塞
// 等待本机用户决策，上限
// [LocalPairingService.incomingPairingDecisionWindow]（60 秒，与引擎侧
// PairingResponder::handle_confirm 的 LOCAL_DECISION_TIMEOUT 一致，锚点
// 都是 hello 到达时刻，见该常量文档）。
//
// 本弹窗职责：展示对端设备名/平台 + SAS 短码，提示用户与对端屏幕核对，
// 按绝对截止时间戳倒计时（归零只关窗、不回传决策——引擎那头此刻同样判定
// 超时，发起方会直接收到 PairingTimeout；主动回传拒绝只会让发起方看到
// 「对方拒绝了配对」这种失实结论），「拒绝」/「确认配对」两个按钮经
// [LocalPairingService.approveIncoming] 回传决策。安全决策不允许点击弹窗
// 外部绕过（barrierDismissible: false）。
//
// 关闭时机统一由 [LocalPairingService.incomingPairing] 变空/变成别的会话
// 驱动（无论是本弹窗内决策、本地倒计时归零兜底关闭，还是对端撤回/出错导致
// 服务层清空该字段）——本弹窗只负责监听并 pop 自己，不在按钮回调里直接调用
// Navigator，避免和外部触发的关闭互相打架。接线位置见 home_page.dart。

import 'dart:async';

import 'package:flutter/material.dart';
import 'package:shadcn_ui/shadcn_ui.dart';

import '../i18n/locale_provider.dart';
import '../services/link/local_pairing_service.dart';
import '../theme/app_colors.dart';
import '../theme/app_metrics.dart';

/// 弹出入站配对核验框。由 home_page 在 `incomingPairing` 变为非空时调用；
/// `barrierDismissible: false` —— 安全决策不允许点击弹窗外部绕过。
Future<void> showIncomingPairingDialog(BuildContext context) {
  return showShadDialog<void>(
    context: context,
    barrierColor: AppColors.of(context).dialogBarrier,
    barrierDismissible: false,
    animateIn: const [],
    animateOut: const [],
    builder: (_) => const IncomingPairingDialog(),
  );
}

/// 入站配对核验框内容。
class IncomingPairingDialog extends StatefulWidget {
  const IncomingPairingDialog({super.key});

  @override
  State<IncomingPairingDialog> createState() => _IncomingPairingDialogState();
}

class _IncomingPairingDialogState extends State<IncomingPairingDialog> {
  Timer? _timer;
  int _secondsLeft = 0;
  bool _closed = false;
  String? _sessionId;
  DateTime? _deadline;

  @override
  void initState() {
    super.initState();
    final svc = LocalPairingService.instance;
    _sessionId = svc.incomingPairing?.sessionId;
    _deadline = svc.incomingPairingDeadline;
    _secondsLeft = _computeSecondsLeft();
    svc.addListener(_onServiceChanged);
    _timer = Timer.periodic(const Duration(seconds: 1), _onTick);
    // 极端时序下打开时会话已失效（服务层状态在弹窗 builder 运行前就被清空）
    // ——下一帧兜底关闭，避免悬空渲染一个指向失效会话的弹窗。
    WidgetsBinding.instance.addPostFrameCallback((_) => _onServiceChanged());
  }

  @override
  void dispose() {
    _timer?.cancel();
    LocalPairingService.instance.removeListener(_onServiceChanged);
    super.dispose();
  }

  /// 按绝对时间戳算剩余秒数（`deadline - now`），不用每次 tick 自减一个
  /// 计数器——后者在系统卡顿/后台限速时会跑偏，导致本地倒计时与引擎侧
  /// 实际决策窗口脱节。[_deadline] 缺失（极端时序下会话已失效）时返回 0。
  int _computeSecondsLeft() {
    final deadline = _deadline;
    if (deadline == null) return 0;
    final leftMs = deadline.difference(DateTime.now()).inMilliseconds;
    return leftMs <= 0 ? 0 : (leftMs / 1000).ceil();
  }

  void _onTick(Timer timer) {
    if (!mounted || _closed) {
      timer.cancel();
      return;
    }
    final left = _computeSecondsLeft();
    if (left <= 0) {
      timer.cancel();
      // 归零只关窗、不回传决策：引擎侧决策窗口与本倒计时同锚在 hello 到达
      // 时刻，归零意味着引擎那头此刻同样判定超时，发起方等待 confirm 会
      // 直接收到 PairingTimeout；主动回传拒绝反而会让发起方看到「对方拒
      // 绝了配对」这种失实结论——没人做出拒绝决策，只是没人来得及看。
      final sessionId = _sessionId;
      if (sessionId != null) {
        LocalPairingService.instance.dismissIncomingPairingTimeout(sessionId);
      }
      return;
    }
    setState(() => _secondsLeft = left);
  }

  /// 会话结束的唯一出口。无论触发源是本弹窗按钮决策
  /// （[LocalPairingService.approveIncoming]）、上面的倒计时归零兜底关闭
  /// （[LocalPairingService.dismissIncomingPairingTimeout]），还是对端撤回
  /// /出错导致服务层清空该字段，都会让 `incomingPairing` 变空或换成别的
  /// sessionId，统一在这里捕获并关闭弹窗——避免多处各自调用 Navigator.pop
  /// 互相冲突。
  void _onServiceChanged() {
    if (_closed || !mounted) return;
    final current = LocalPairingService.instance.incomingPairing;
    if (current != null && current.sessionId == _sessionId) return;
    _closed = true;
    _timer?.cancel();
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (mounted) Navigator.of(context).maybePop();
    });
  }

  @override
  Widget build(BuildContext context) {
    final s = LocaleScope.of(context);
    final c = AppColors.of(context);
    final m = AppMetrics.of(context);
    final pairing = LocalPairingService.instance.incomingPairing;
    // 会话已失效但 _onServiceChanged 的兜底 pop 还未生效——渲染空壳过渡帧。
    if (pairing == null) return const SizedBox.shrink();
    final spaced = pairing.sas.split('').join('  ');
    return ShadDialog(
      title: Text(s.incomingPairingTitle),
      description: Text(s.incomingPairingFrom(pairing.peerName)),
      actions: [
        ShadButton.outline(
          onPressed: () => LocalPairingService.instance.approveIncoming(false),
          child: Text(s.incomingPairingReject),
        ),
        ShadButton(
          onPressed: () => LocalPairingService.instance.approveIncoming(true),
          child: Text(s.incomingPairingAccept),
        ),
      ],
      child: Padding(
        padding: const EdgeInsets.symmetric(vertical: 8),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          mainAxisSize: MainAxisSize.min,
          children: [
            Row(
              children: [
                Icon(
                  _platformIcon(pairing.peerPlatform),
                  size: 14,
                  color: c.textMuted,
                ),
                const SizedBox(width: 6),
                Text(
                  _platformLabel(s, pairing.peerPlatform),
                  style: TextStyle(fontSize: 12, color: c.textMuted),
                ),
              ],
            ),
            const SizedBox(height: 14),
            Container(
              padding: const EdgeInsets.symmetric(vertical: 18),
              decoration: BoxDecoration(
                color: c.surface2,
                borderRadius: m.brInput,
              ),
              alignment: Alignment.center,
              child: Text(
                spaced,
                style: TextStyle(
                  fontSize: 26,
                  fontWeight: FontWeight.w700,
                  letterSpacing: 2,
                  color: c.textPrimary,
                  fontFeatures: const [FontFeature.tabularFigures()],
                ),
              ),
            ),
            const SizedBox(height: 10),
            Text(
              s.incomingPairingHint,
              style: TextStyle(fontSize: 11.5, height: 1.5, color: c.textMuted),
            ),
            const SizedBox(height: 8),
            Text(
              s.incomingPairingCountdown(_secondsLeft),
              style: TextStyle(
                fontSize: 11,
                fontWeight: FontWeight.w600,
                color: c.statusWarning,
              ),
            ),
          ],
        ),
      ),
    );
  }
}

// 与 add_device_dialog.dart 的同名私有 helper 重复——Dart 私有标识符按文件
// 隔离，无法跨文件复用；两处逻辑均为 6 行内的简单 switch，不值得为此新增
// 共享抽象文件。

IconData _platformIcon(String platform) => switch (platform) {
      'windows' || 'macos' || 'linux' => LucideIcons.monitor,
      'android' || 'ios' => LucideIcons.smartphone,
      'server' => LucideIcons.server,
      _ => LucideIcons.server,
    };

String _platformLabel(S s, String platform) => switch (platform) {
      'windows' => s.accountDevicePlatformWindows,
      'macos' => s.accountDevicePlatformMacos,
      'linux' => s.accountDevicePlatformLinux,
      'android' => s.accountDevicePlatformAndroid,
      'ios' => s.accountDevicePlatformIos,
      _ => '—',
    };
