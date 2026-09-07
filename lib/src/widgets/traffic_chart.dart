// 实时流量折线图（迅雷风格）。放置于详情页「下载分布」上方。
//
// 交互（参照迅雷/主流下载器速率图）：
// - 滚轮：缩放时间轴（以鼠标所在时刻为锚点）；
// - Ctrl/⌘ + 滚轮：缩放纵轴（速率刻度）；
// - 鼠标按住水平拖动：平移时间窗口；离开最右端后暂停自动跟随，
//   拖回最右端或新数据超过窗口时恢复跟随最右端。
//
// 数据：[TrafficSample]（下载/上传 bps，约 1s 一个样本），由
// DownloadController 在任务活跃期采集。

import 'package:flutter/gestures.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';

import '../models/download_controller.dart';
import '../theme/app_colors.dart';

/// 下载曲线颜色（与分段进度同蓝，保证全 App 语义一致）。
const Color kTrafficDown = Color(0xFF2563EB);
/// 上传曲线颜色（BT 做种/上传用橙，区分于下载蓝）。
const Color kTrafficUp = Color(0xFFF59E0B);

class TrafficChart extends StatefulWidget {
  final List<TrafficSample> samples;
  final bool showUp;
  final String title;
  final String downLabel;
  final String upLabel;
  final double height;

  const TrafficChart({
    super.key,
    required this.samples,
    required this.title,
    required this.downLabel,
    required this.upLabel,
    this.showUp = false,
    this.height = 132,
  });

  @override
  State<TrafficChart> createState() => _TrafficChartState();
}

class _TrafficChartState extends State<TrafficChart> {
  /// 时间窗口宽度（ms）。默认 120s。
  double _spanMs = 120 * 1000;
  static const double _minSpanMs = 10 * 1000;
  static const double _maxSpanMs = 3600 * 1000;

  /// 是否自动跟随最右端（新样本进来窗口随之右移）。
  bool _autoFollow = true;

  /// 用户手动平移/缩放后锁定的右端时刻（ms）；-1 = 未锁定。
  double _rightMs = -1;

  /// 纵轴缩放因子：1 = 按数据自动适配；<1 放大细节；>1 抬高满刻度。
  double _yFactor = 1.0;
  static const double _yMinFactor = 0.2;
  static const double _yMaxFactor = 8.0;

  double get _lastMs {
    final s = widget.samples;
    if (s.isEmpty) return 0;
    return s.last.ms.toDouble();
  }

  @override
  void didUpdateWidget(TrafficChart old) {
    super.didUpdateWidget(old);
    if (!identical(old.samples, widget.samples) &&
        _autoFollow &&
        widget.samples.isNotEmpty) {
      // 新样本到达：右端跟随数据，触发重绘让曲线平滑右移。
      setState(() {});
    }
  }

  void _handleSignal(PointerSignalEvent event) {
    if (event is! PointerScrollEvent) return;
    // 注册进 pointerSignalResolver：resolver 只调用「最先注册」的回调，而最先
    // 注册的正是层级里最深的 widget——本图表比详情页外层 Scrollable 更深，因此
    // 会赢得这个滚轮事件。若不注册，页面 Scrollable 会照常收到同一事件去滚页面，
    // 造成「滚图表缩放的同时页面也在滚」的双滚动。
    GestureBinding.instance.pointerSignalResolver.register(event, _applyScroll);
  }

  void _applyScroll(PointerSignalEvent event) {
    final scroll = event as PointerScrollEvent;
    final dy = scroll.scrollDelta.dy;
    if (dy == 0) return;
    final ctrl = HardwareKeyboard.instance.isControlPressed ||
        HardwareKeyboard.instance.isMetaPressed;

    final renderBox = context.findRenderObject() as RenderBox?;
    if (renderBox == null || !renderBox.hasSize) return;
    final local = renderBox.globalToLocal(scroll.position);
    final frac = (local.dx / renderBox.size.width).clamp(0.0, 1.0);

    if (!ctrl) {
      // 时间轴缩放：以指针时刻为锚点。
      final oldSpan = _spanMs;
      final factor = dy > 0 ? 1.2 : 1 / 1.2;
      final newSpan = (oldSpan * factor).clamp(_minSpanMs, _maxSpanMs);
      if (newSpan == oldSpan) return;
      final right = _effectiveRight();
      final anchorMs = right - (1 - frac) * oldSpan;
      setState(() {
        _spanMs = newSpan;
        _rightMs = (anchorMs + (1 - frac) * newSpan).clamp(0.0, double.infinity);
        _autoFollow = false;
        _maybeRestoreFollow();
      });
    } else {
      // 纵轴缩放（Ctrl/⌘ + 滚轮）。
      final factor = dy > 0 ? 1.2 : 1 / 1.2;
      setState(() {
        _yFactor = (_yFactor * factor).clamp(_yMinFactor, _yMaxFactor);
      });
    }
  }

  void _onPanStart(DragStartDetails d) => _autoFollow = false;

  void _onPanUpdate(DragUpdateDetails d) {
    final renderBox = context.findRenderObject() as RenderBox?;
    if (renderBox == null || !renderBox.hasSize) return;
    final msPerPx = _spanMs / renderBox.size.width;
    setState(() {
      _rightMs = (_effectiveRight() + d.delta.dx * msPerPx)
          .clamp(0.0, double.infinity);
      _maybeRestoreFollow();
    });
  }

  /// 右端有效值：未锁定则跟随最新样本（带 2% 余量）。
  double _effectiveRight() {
    final last = _lastMs;
    if (!_autoFollow) {
      if (_rightMs >= 0) return _rightMs;
      _autoFollow = true;
    }
    return last + _spanMs * 0.02;
  }

  /// 绘制用右端（ms）：自动跟随且数据不足一窗时至少撑满 [span]，避免负时间标签。
  double _paintRightMs() {
    final span = _spanMs;
    if (_autoFollow) {
      final last = _lastMs;
      return last + span * 0.02 < span ? span : last + span * 0.02;
    }
    return _rightMs >= 0 ? _rightMs : span;
  }

  /// 绘制用左端（ms）：不小于 0。
  double _paintLeftMs() {
    final l = _paintRightMs() - _spanMs;
    return l < 0 ? 0 : l;
  }

  /// 若右端已贴近最新数据（<1s），恢复自动跟随。
  void _maybeRestoreFollow() {
    if (_rightMs >= 0 && (_lastMs - _rightMs).abs() < 1000) {
      _autoFollow = true;
      _rightMs = -1;
    }
  }

  @override
  Widget build(BuildContext context) {
    final c = AppColors.of(context);
    return Container(
      decoration: BoxDecoration(
        color: c.surface1,
        borderRadius: BorderRadius.circular(10),
        border: Border.all(color: c.border, width: 1),
      ),
      padding: const EdgeInsets.fromLTRB(12, 10, 12, 8),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Row(
            children: [
              Text(
                widget.title,
                style: TextStyle(
                  fontSize: 12,
                  fontWeight: FontWeight.w600,
                  color: c.textPrimary,
                ),
              ),
              const SizedBox(width: 10),
              _LegendDot(color: kTrafficDown, label: widget.downLabel, c: c),
              if (widget.showUp) ...[
                const SizedBox(width: 10),
                _LegendDot(color: kTrafficUp, label: widget.upLabel, c: c),
              ],
              const Spacer(),
              Text(
                '滚轮缩放 · Ctrl+滚轮调纵轴',
                style: TextStyle(fontSize: 10, color: c.textMuted),
              ),
            ],
          ),
          const SizedBox(height: 8),
          SizedBox(
            height: widget.height,
            width: double.infinity,
            child: Listener(
              onPointerSignal: _handleSignal,
              child: GestureDetector(
                behavior: HitTestBehavior.opaque,
                onHorizontalDragStart: _onPanStart,
                onHorizontalDragUpdate: _onPanUpdate,
                child: ClipRect(
                  child: CustomPaint(
                    painter: _TrafficPainter(
                      samples: widget.samples,
                      showUp: widget.showUp,
                      leftMs: _paintLeftMs(),
                      rightMs: _paintRightMs(),
                      yFactor: _yFactor,
                      c: c,
                    ),
                  ),
                ),
              ),
            ),
          ),
        ],
      ),
    );
  }
}

class _LegendDot extends StatelessWidget {
  final Color color;
  final String label;
  final AppColors c;
  const _LegendDot({
    required this.color,
    required this.label,
    required this.c,
  });

  @override
  Widget build(BuildContext context) {
    return Row(
      mainAxisSize: MainAxisSize.min,
      children: [
        Container(
          width: 8,
          height: 8,
          decoration: BoxDecoration(color: color, shape: BoxShape.circle),
        ),
        const SizedBox(width: 4),
        Text(label, style: TextStyle(fontSize: 10.5, color: c.textSecondary)),
      ],
    );
  }
}

class _TrafficPainter extends CustomPainter {
  final List<TrafficSample> samples;
  final bool showUp;
  final double leftMs;
  final double rightMs;
  final double yFactor;
  final AppColors c;

  static const double _axisLeft = 56;
  static const double _axisRight = 6;
  static const double _axisTop = 8;
  static const double _axisBottom = 20;

  _TrafficPainter({
    required this.samples,
    required this.showUp,
    required this.leftMs,
    required this.rightMs,
    required this.yFactor,
    required this.c,
  });

  @override
  void paint(Canvas canvas, Size size) {
    final w = size.width;
    final h = size.height;
    final plotW = (w - _axisLeft - _axisRight).clamp(10.0, double.infinity);
    final plotH = (h - _axisTop - _axisBottom).clamp(10.0, double.infinity);
    final plot = Rect.fromLTWH(_axisLeft, _axisTop, plotW, plotH);
    final span = (rightMs - leftMs).clamp(1.0, double.infinity);

    // 背景 + 边框。
    canvas.drawRect(
      Offset.zero & size,
      Paint()..color = c.surface2.withValues(alpha: 0.6),
    );
    final border = Paint()
      ..color = c.border
      ..style = PaintingStyle.stroke
      ..strokeWidth = 1;
    canvas.drawRect(Offset.zero & size, border);

    if (samples.isEmpty) {
      _text(
        canvas,
        '暂无流量数据',
        Offset(w / 2, h / 2),
        Paint()..color = c.textMuted,
        alignCenter: true,
      );
      return;
    }

    // 可见样本的自动峰值。
    var peak = 0.0;
    for (final s in samples) {
      if (s.ms < leftMs || s.ms > rightMs) continue;
      if (s.down > peak) peak = s.down.toDouble();
      if (showUp && s.up > peak) peak = s.up.toDouble();
    }
    if (peak <= 0) peak = 1;
    peak = peak * 1.1 * yFactor;

    // Y 网格（约 4 条）。
    final yMaxNice = _niceCeil(peak, 4);
    final yTicks = 4;
    final gridPaint = Paint()
      ..color = c.border.withValues(alpha: 0.6)
      ..strokeWidth = 1;
    for (var i = 0; i <= yTicks; i++) {
      final frac = i / yTicks;
      final y = plot.bottom - frac * plotH;
      canvas.drawLine(Offset(plot.left, y), Offset(plot.right, y), gridPaint);
      final val = yMaxNice * frac;
      _text(
        canvas,
        _fmtBps(val),
        Offset(_axisLeft - 6, y - 6),
        Paint()..color = c.textMuted,
        alignRight: true,
      );
    }

    // X 时间刻度（约 6 条）。
    final xTicks = 6;
    for (var i = 0; i <= xTicks; i++) {
      final frac = i / xTicks;
      final x = plot.left + frac * plotW;
      canvas.drawLine(Offset(x, plot.top), Offset(x, plot.bottom), gridPaint);
      final tMs = leftMs + span * frac;
      _text(
        canvas,
        _fmtClock(tMs.round()),
        Offset(x, plot.bottom + 4),
        Paint()..color = c.textMuted,
        alignCenter: true,
      );
    }

    canvas.save();
    canvas.clipRect(plot);

    double xOf(TrafficSample s) =>
        plot.left + ((s.ms - leftMs) / span) * plotW;
    double yOf(double bps) =>
        plot.bottom - ((bps / yMaxNice).clamp(0.0, 1.0)) * plotH;

    // 下载线（蓝）+ 面积。
    _polyline(
      canvas,
      samples,
      xOf,
      yOf,
      plot.bottom,
      kTrafficDown,
      fill: true,
    );
    // 上传线（橙）。
    if (showUp) {
      _polyline(
        canvas,
        samples,
        xOf,
        yOf,
        plot.bottom,
        kTrafficUp,
        fill: false,
      );
    }

    canvas.restore();
  }

  /// 绘制一条速率折线；[baselineY] 为图底 y，供面积填充收口。
  void _polyline(
    Canvas canvas,
    List<TrafficSample> all,
    double Function(TrafficSample) xOf,
    double Function(double) yOf,
    double baselineY,
    Color color, {
    required bool fill,
  }) {
    // 窗口内样本（样本按 ms 递增）。
    final visible = <TrafficSample>[];
    for (final s in all) {
      if (s.ms >= leftMs && s.ms <= rightMs) visible.add(s);
    }
    if (visible.isEmpty) return;

    final line = Path();
    for (var i = 0; i < visible.length; i++) {
      final x = xOf(visible[i]);
      final y = yOf(visible[i].down.toDouble());
      if (i == 0) {
        line.moveTo(x, y);
      } else {
        line.lineTo(x, y);
      }
    }
    if (fill) {
      final area = Path.from(line)
        ..lineTo(xOf(visible.last), baselineY)
        ..lineTo(xOf(visible.first), baselineY)
        ..close();
      canvas.drawPath(
        area,
        Paint()..color = color.withValues(alpha: 0.12),
      );
    }
    canvas.drawPath(
      line,
      Paint()
        ..color = color
        ..style = PaintingStyle.stroke
        ..strokeWidth = 1.6
        ..strokeJoin = StrokeJoin.round
        ..strokeCap = StrokeCap.round,
    );
  }

  void _text(
    Canvas canvas,
    String text,
    Offset at,
    Paint paint, {
    bool alignRight = false,
    bool alignCenter = false,
  }) {
    final tp = TextPainter(
      text: TextSpan(
        text: text,
        style: TextStyle(
          fontSize: 9.5,
          color: paint.color,
          fontFeatures: const [FontFeature.tabularFigures()],
        ),
      ),
      textDirection: TextDirection.ltr,
    )..layout();
    var dx = at.dx;
    if (alignRight) dx -= tp.width;
    if (alignCenter) dx -= tp.width / 2;
    tp.paint(canvas, Offset(dx, at.dy));
  }

  String _fmtBps(double bps) {
    if (bps >= 1024 * 1024) {
      return '${(bps / (1024 * 1024)).toStringAsFixed(1)} MB/s';
    }
    if (bps >= 1024) return '${(bps / 1024).round()} KB/s';
    return '${bps.round()} B/s';
  }

  String _fmtClock(int ms) {
    final totalSec = ms ~/ 1000;
    final h = totalSec ~/ 3600;
    final m = (totalSec % 3600) ~/ 60;
    final sec = totalSec % 60;
    String p2(int n) => n.toString().padLeft(2, '0');
    if (h > 0) return '$h:${p2(m)}:${p2(sec)}';
    return '${p2(m)}:${p2(sec)}';
  }

  double _niceCeil(double v, int ticks) {
    if (v <= 0) return 1;
    final rough = v / ticks;
    final double mag = 1;
    var power = mag;
    while (power * 10 <= rough) {
      power *= 10;
    }
    double mult = power;
    for (final m in [1.0, 2.0, 5.0, 10.0]) {
      final candidate = m * power;
      if (candidate >= rough) {
        mult = candidate;
        break;
      }
    }
    return mult * ticks;
  }

  @override
  bool shouldRepaint(_TrafficPainter old) =>
      !identical(samples, old.samples) ||
      showUp != old.showUp ||
      leftMs != old.leftMs ||
      rightMs != old.rightMs ||
      yFactor != old.yFactor ||
      c != old.c;
}
