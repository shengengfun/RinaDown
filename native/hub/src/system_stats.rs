// 性能监控采样（Windows，状态栏「限速」左侧的 CPU/内存/网络）。
//
// 每 2s 一次：CPU 用 GetSystemTimes 差值（idle/kernel+user），内存直接取
// GlobalMemoryStatusEx.dwMemoryLoad（0-100），网络用 GetIfTable2 各网卡
// InOctets/OutOctets 差值与时间间隔折算 bps。采样是同步 Win32 调用，按
// 本 crate 运行时约束放进 spawn_blocking；差值状态放进程级 Mutex。
//
// 只在 Windows 编译/运行；其它平台无此模块（状态栏相应隐藏）。

use std::sync::Mutex;
use std::time::Instant;

use rinf::RustSignal;
use windows_sys::Win32::Foundation::FILETIME;
use windows_sys::Win32::NetworkManagement::IpHelper::{FreeMibTable, GetIfTable2, MIB_IF_TABLE2};
use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
use windows_sys::Win32::System::Threading::GetSystemTimes;

const POLL_SECS: u64 = 2;

/// 上次采样值（进程级单例；跨线程仅 short 临界）。
struct Prev {
    idle: u64,
    kernel: u64,
    user: u64,
    rx: u64,
    tx: u64,
    instant: Instant,
}

static PREV: Mutex<Option<Prev>> = Mutex::new(None);

#[inline]
fn ft(ts: FILETIME) -> u64 {
    ((ts.dwHighDateTime as u64) << 32) | ts.dwLowDateTime as u64
}

/// 读一次快照 → (cpu%, ram%, rx_bps, tx_bps)。非 Windows / 采样失败返回 None。
fn sample() -> Option<(f32, f32, f32, f32)> {
    unsafe {
        let mut idle = FILETIME { dwLowDateTime: 0, dwHighDateTime: 0 };
        let mut kernel = FILETIME { dwLowDateTime: 0, dwHighDateTime: 0 };
        let mut user = FILETIME { dwLowDateTime: 0, dwHighDateTime: 0 };
        if GetSystemTimes(&mut idle, &mut kernel, &mut user) == 0 {
            return None;
        }
        let mut msx = MEMORYSTATUSEX {
            dwLength: std::mem::size_of::<MEMORYSTATUSEX>() as u32,
            ..std::mem::zeroed()
        };
        if GlobalMemoryStatusEx(&mut msx) == 0 {
            return None;
        }
        let ram = msx.dwMemoryLoad as f32;
        let (rx, tx) = net_bytes().unwrap_or((0, 0));
        let now = Instant::now();

        let idle = ft(idle);
        let kernel = ft(kernel);
        let user = ft(user);

        let mut cpu = 0.0f32;
        let mut rx_bps = 0.0f32;
        let mut tx_bps = 0.0f32;

        let mut guard = PREV.lock().unwrap();
        if let Some(p) = guard.take() {
            let dt = now.duration_since(p.instant).as_secs_f32().max(0.05);
            let didle = idle.saturating_sub(p.idle) as f32;
            let dtotal = (kernel.saturating_sub(p.kernel) + user.saturating_sub(p.user)) as f32;
            if dtotal > 0.0 {
                cpu = ((dtotal - didle) / dtotal * 100.0).clamp(0.0, 100.0);
            }
            rx_bps = rx.saturating_sub(p.rx) as f32 / dt;
            tx_bps = tx.saturating_sub(p.tx) as f32 / dt;
        }
        *guard = Some(Prev { idle, kernel, user, rx, tx, instant: now });
        Some((cpu, ram, rx_bps, tx_bps))
    }
}

/// 汇总所有网卡收发字节数。
unsafe fn net_bytes() -> Option<(u64, u64)> {
    let mut table: *mut MIB_IF_TABLE2 = std::ptr::null_mut();
    if GetIfTable2(&mut table) != 0 || table.is_null() {
        return None;
    }
    let mut rx = 0u64;
    let mut tx = 0u64;
    let t = &*table;
    let rows = std::slice::from_raw_parts(t.Table.as_ptr(), t.NumEntries as usize);
    for row in rows {
        rx = rx.wrapping_add(row.InOctets);
        tx = tx.wrapping_add(row.OutOctets);
    }
    FreeMibTable(table as *const _);
    Some((rx, tx))
}

/// 启动周期采样并向 Dart 发 SystemStats。
pub fn spawn() {
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(std::time::Duration::from_secs(POLL_SECS));
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tick.tick().await;
            let res = tokio::task::spawn_blocking(sample).await.unwrap_or(None);
            if let Some((cpu, ram, rx_bps, tx_bps)) = res {
                crate::signals::SystemStats {
                    cpu_percent: cpu,
                    ram_percent: ram,
                    net_down_bps: rx_bps as u64,
                    net_up_bps: tx_bps as u64,
                }
                .send_signal_to_dart();
            }
        }
    });
}
