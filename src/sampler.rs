//! 系统采样：CPU / 内存（含 VM 分类统计）/ 网络 / 进程与网卡明细。
//!
//! 所有状态只在主线程读写（NSTimer 回调与 GPUI 渲染都在主线程），
//! 唯一例外：网络详情的后台 nettop 监视线程（见 `ensure_net_monitor`）。
//! 采样频率 2s；进程/网卡明细仅在详情视图打开时才枚举，守住常驻占用。

use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::ffi::CStr;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use objc2_core_foundation::{CFDictionary, CFNumber, CFNumberType, CFString};
use sysinfo::{Networks, Pid, ProcessesToUpdate, System};

use crate::fmt::fmt_speed;

/// 采样间隔（秒）
pub const TICK_SECS: f64 = 2.0;

pub(crate) struct Samples {
    sys: System,
    nets: Networks,
}

#[derive(Clone, Copy, Default, Debug)]
pub struct Snapshot {
    /// CPU 总使用率（用户态 + 系统态）
    pub cpu: f32,
    /// CPU 用户态 / 系统态占比（面板双折线拆分用）
    pub cpu_user: f32,
    pub cpu_sys: f32,
    pub mem_pct: f64,
    pub mem_used_gb: f64,
    pub mem_total_gb: f64,
    /// 联动（wired）/ 活跃（active）/ 已压缩（compressed）/ 实际可用，单位 GB
    pub wired_gb: f64,
    pub active_gb: f64,
    pub compressed_gb: f64,
    pub avail_gb: f64,
    pub rx: f64,
    pub tx: f64,
    /// 磁盘读 / 写速率（B/s，整盘汇总）
    pub disk_read: f64,
    pub disk_write: f64,
}

/// 主线程（NSTimer 回调）写入，GPUI 渲染线程/主线程读取
static SNAPSHOT: Mutex<Snapshot> = Mutex::new(Snapshot {
    cpu: 0.0,
    cpu_user: 0.0,
    cpu_sys: 0.0,
    mem_pct: 0.0,
    mem_used_gb: 0.0,
    mem_total_gb: 0.0,
    wired_gb: 0.0,
    active_gb: 0.0,
    compressed_gb: 0.0,
    avail_gb: 0.0,
    rx: 0.0,
    tx: 0.0,
    disk_read: 0.0,
    disk_write: 0.0,
});

/// 当前快照（克隆）
pub fn snapshot() -> Snapshot {
    *SNAPSHOT.lock().unwrap()
}

/// 趋势历史（每指标最多 60 个点 ≈ 2 分钟）
#[derive(Clone, Default)]
pub struct Hist {
    /// CPU 总使用率（菜单栏迷你趋势图用）
    pub cpu: VecDeque<f32>,
    /// CPU 用户态 / 系统态占比（面板双折线用）
    pub cpu_user: VecDeque<f32>,
    pub cpu_sys: VecDeque<f32>,
    pub mem: VecDeque<f32>,
    /// 磁盘读 / 写速率（B/s，磁盘详情蝶形图用）
    pub disk_read: VecDeque<f32>,
    pub disk_write: VecDeque<f32>,
}

static HIST: Mutex<Hist> = Mutex::new(Hist {
    cpu: VecDeque::new(),
    cpu_user: VecDeque::new(),
    cpu_sys: VecDeque::new(),
    mem: VecDeque::new(),
    disk_read: VecDeque::new(),
    disk_write: VecDeque::new(),
});

pub fn hist() -> Hist {
    HIST.lock().unwrap().clone()
}

/// 详情视图行（图标在渲染时按 pid+exe 惰性提取并缓存）
#[derive(Clone)]
pub struct DetailRow {
    pub name: String,
    pub value: String,
    pub pid: Option<u32>,
    pub exe: Option<PathBuf>,
}

static DETAIL_ROWS: Mutex<Vec<DetailRow>> = Mutex::new(Vec::new());

/// 网络监视线程发布的每进程速率（后台写、主线程读）
#[derive(Clone)]
pub(crate) struct NetStat {
    pub name: String,
    pub pid: u32,
    pub text: String,
}
static NET_STATS: Mutex<Vec<NetStat>> = Mutex::new(Vec::new());

/// 磁盘明细上一轮基线（pid → 累计读写字节），差分出每进程速率
static DISK_PROC_PREV: Mutex<Option<HashMap<u32, (u64, u64)>>> = Mutex::new(None);

pub fn detail_rows() -> Vec<DetailRow> {
    DETAIL_ROWS.lock().unwrap().clone()
}

/// 当前详情资源：Some(0)=CPU、Some(1)=内存、Some(2)=网络；None=不在详情页（不枚举明细）
static ACTIVE: Mutex<Option<u8>> = Mutex::new(None);

/// 详情打开后的采样轮数：进程 CPU 使用率是两次刷新的差分，第一轮全为 0
static ACTIVE_TICKS: Mutex<u32> = Mutex::new(0);

thread_local! {
    /// System::new() 而非 new_all()：后者会枚举全部进程，常驻内存翻倍，而本应用用不到
    static SAMPLES: RefCell<Samples> = RefCell::new(Samples {
        sys: System::new(),
        nets: Networks::new_with_refreshed_list(),
    });
    /// 上一轮 CPU tick 基线（user / system / idle / nice），差分出用户态与系统态占比
    static CPU_TICKS_PREV: RefCell<Option<[u64; 4]>> = const { RefCell::new(None) };
    /// 上一轮磁盘读写累计基线（自启动以来的总字节数 + 采样时刻），差分出读写速率
    static DISK_IO_PREV: RefCell<Option<(u64, u64, Instant)>> = const { RefCell::new(None) };
}

/// host_statistics64 取 CPU tick，与上一轮差分出 (用户态%, 系统态%)。
/// nice 属用户发起的低优先级时间，并入用户态（与系统活动监视器口径一致）。
/// 首轮无基线返回 (0.0, 0.0)，与进程明细的热身逻辑一致。
#[allow(deprecated)] // libc 的 mach_host_self 标记废弃，项目既有采样同样如此
fn cpu_usage_split() -> (f32, f32) {
    unsafe {
        let port = libc::mach_host_self();
        let mut count: u32 = libc::HOST_CPU_LOAD_INFO_COUNT;
        let mut stat = std::mem::zeroed::<libc::host_cpu_load_info>();
        if libc::host_statistics64(
            port,
            libc::HOST_CPU_LOAD_INFO,
            &mut stat as *mut libc::host_cpu_load_info as *mut _,
            &mut count,
        ) != libc::KERN_SUCCESS
        {
            return (0.0, 0.0);
        }
        let ticks: [u64; 4] = stat.cpu_ticks.map(|t| t as u64);
        let Some(prev) = CPU_TICKS_PREV.with(|c| c.borrow_mut().replace(ticks)) else {
            return (0.0, 0.0);
        };
        let d = |i: usize| ticks[i].saturating_sub(prev[i]);
        let (user, sys, idle, nice) = (d(0), d(1), d(2), d(3));
        let total = user + nice + sys + idle;
        if total == 0 {
            return (0.0, 0.0);
        }
        let user_pct = (user + nice) as f32 / total as f32 * 100.0;
        let sys_pct = sys as f32 / total as f32 * 100.0;
        (user_pct, sys_pct)
    }
}

// ---------- 磁盘读写：IOKit 块设备驱动注册表统计 ----------

// IOKit 的 C 接口不在 objc2 绑定内（同 CGBitmapContextCreate 的处理），extern shim 声明
#[link(name = "IOKit", kind = "framework")]
extern "C" {
    fn IOServiceMatching(className: *const std::os::raw::c_char) -> *mut std::os::raw::c_void;
    fn IOServiceGetMatchingServices(
        main_port: libc::mach_port_t,
        matching: *mut std::os::raw::c_void,
        existing: *mut libc::mach_port_t,
    ) -> libc::kern_return_t;
    fn IOIteratorNext(iterator: libc::mach_port_t) -> libc::mach_port_t;
    fn IORegistryEntryCreateCFProperties(
        entry: libc::mach_port_t,
        properties: *mut *mut std::os::raw::c_void,
        allocator: *mut std::os::raw::c_void,
        options: libc::c_uint,
    ) -> libc::kern_return_t;
    fn IOObjectRelease(object: libc::mach_port_t) -> libc::kern_return_t;
}

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    fn CFRelease(cf: *mut std::os::raw::c_void);
}

/// 聚合一类块设备驱动的累计读写字节数（自系统启动以来）；无匹配设备返回 None
unsafe fn disk_io_bytes_for(class: &CStr) -> Option<(u64, u64)> {
    let matching = IOServiceMatching(class.as_ptr());
    if matching.is_null() {
        return None;
    }
    let mut iter: libc::mach_port_t = 0;
    if IOServiceGetMatchingServices(0, matching, &mut iter) != libc::KERN_SUCCESS {
        return None;
    }
    let (mut read, mut write) = (0u64, 0u64);
    let mut found = false;
    loop {
        let obj = IOIteratorNext(iter);
        if obj == 0 {
            break;
        }
        found = true;
        let mut props: *mut std::os::raw::c_void = std::ptr::null_mut();
        if IORegistryEntryCreateCFProperties(obj, &mut props, std::ptr::null_mut(), 0)
            == libc::KERN_SUCCESS
            && !props.is_null()
        {
            let dict = &*(props as *const CFDictionary);
            let stats_key = CFString::from_str("Statistics");
            let stats = dict.value(&*stats_key as *const CFString as *const _);
            if !stats.is_null() {
                let stats = &*(stats as *const CFDictionary);
                for (key, total) in [("Bytes (Read)", &mut read), ("Bytes (Write)", &mut write)] {
                    let k = CFString::from_str(key);
                    let num = stats.value(&*k as *const CFString as *const _);
                    if !num.is_null() {
                        let mut v: u64 = 0;
                        let num = &*(num as *const CFNumber);
                        if num.value(CFNumberType::SInt64Type, &mut v as *mut u64 as *mut _) {
                            *total += v;
                        }
                    }
                }
            }
            CFRelease(props);
        }
        IOObjectRelease(obj);
    }
    IOObjectRelease(iter);
    found.then_some((read, write))
}

/// 磁盘读写字节数：先查传统块设备驱动，找不到再查 NVMe 驱动
/// （两类分开查询避免同一块盘被继承匹配重复统计）
fn disk_io_bytes() -> Option<(u64, u64)> {
    unsafe {
        disk_io_bytes_for(c"IOBlockStorageDriver")
            .or_else(|| disk_io_bytes_for(c"IONVMeBlockStorageDriver"))
    }
}

/// 磁盘读 / 写速率（B/s）：注册表累计字节差分 / 采样间隔；首轮无基线为 0
fn disk_rates() -> (f64, f64) {
    static MISSED: AtomicBool = AtomicBool::new(false);
    let now = Instant::now();
    let Some((read, write)) = disk_io_bytes() else {
        DISK_IO_PREV.with(|c| *c.borrow_mut() = None);
        if !MISSED.swap(true, Ordering::SeqCst) {
            eprintln!("[statbar] IOKit 未找到块设备驱动读写统计，磁盘速率显示为 0");
        }
        return (0.0, 0.0);
    };
    let prev = DISK_IO_PREV.with(|c| c.borrow_mut().replace((read, write, now)));
    let Some((pr, pw, t0)) = prev else {
        return (0.0, 0.0);
    };
    let dt = now.duration_since(t0).as_secs_f64().max(0.001);
    (
        read.saturating_sub(pr) as f64 / dt,
        write.saturating_sub(pw) as f64 / dt,
    )
}

/// 长名称截断（超出宽度限制时省略号收尾）
fn truncate(name: String, max: usize) -> String {
    if name.chars().count() > max {
        format!("{}…", name.chars().take(max - 1).collect::<String>())
    } else {
        name
    }
}

/// 切换详情资源：清空上一视图的缓存行 + 重置热身计数 + 管理网络监视线程
pub fn set_active(kind: Option<u8>) {
    {
        let mut active = ACTIVE.lock().unwrap();
        if *active == kind {
            return;
        }
        *active = kind;
    }
    // 切换后旧列表属于上一个视图，立即清空避免「缓存」误导
    *DETAIL_ROWS.lock().unwrap() = Vec::new();
    *ACTIVE_TICKS.lock().unwrap() = 0;
    *DISK_PROC_PREV.lock().unwrap() = None;
    if kind == Some(2) {
        ensure_net_monitor();
    }
}

// ---------- 网络详情：nettop 后台监视线程 ----------

static NET_MON_ON: AtomicBool = AtomicBool::new(false);
/// nettop 上一轮快照（进程键 → 累计收发字节）
static NET_PREV: Mutex<Option<(Instant, HashMap<String, (u64, u64)>)>> =
    Mutex::new(None);

/// 网络详情打开时确保监视线程在跑（nettop 子进程轮询 + 差分出每进程速率）
fn ensure_net_monitor() {
    if NET_MON_ON.swap(true, Ordering::SeqCst) {
        return;
    }
    std::thread::spawn(|| {
        loop {
            if !NET_MON_ON.load(Ordering::SeqCst)
                || ACTIVE.lock().unwrap().unwrap_or(255) != 2
            {
                break;
            }
            let out = std::process::Command::new("nettop")
                .args(["-P", "-L", "1", "-n", "-J", "bytes_in,bytes_out"])
                .output();
            let Ok(out) = out else { break };

            // 解析 CSV：进程.pid, bytes_in, bytes_out
            let text = String::from_utf8_lossy(&out.stdout);
            let mut cur: HashMap<String, (u64, u64)> = HashMap::new();
            for line in text.lines() {
                let parts: Vec<&str> = line.split(',').collect();
                if parts.len() >= 3 && !parts[0].is_empty() {
                    let bin: u64 = parts[1].parse().unwrap_or(0);
                    let bout: u64 = parts[2].parse().unwrap_or(0);
                    cur.insert(parts[0].to_string(), (bin, bout));
                }
            }

            let now = Instant::now();
            let prev = std::mem::take(&mut *NET_PREV.lock().unwrap());
            let mut rows: Vec<(String, u32, String, f64)> = Vec::new();
            if let Some((t0, prev_map)) = prev {
                let dt = t0.elapsed().as_secs_f64().max(0.001);
                for (key, (bin, bout)) in &cur {
                    let Some((pin, pout)) = prev_map.get(key) else {
                        continue;
                    };
                    let rin = bin.saturating_sub(*pin);
                    let rout = bout.saturating_sub(*pout);
                    if rin == 0 && rout == 0 {
                        continue;
                    }
                    let (name, pid) = split_key(key);
                    if name == "statbar" {
                        continue;
                    }
                    rows.push((
                        truncate(name, 30),
                        pid,
                        format!("↓{} ↑{}", fmt_speed(rin as f64 / dt), fmt_speed(rout as f64 / dt)),
                        rin as f64 + rout as f64,
                    ));
                }
                rows.sort_by(|a, b| b.3.partial_cmp(&a.3).unwrap_or(std::cmp::Ordering::Equal));
            }
            // 本轮快照成为下一轮基线；速率发布到 NET_STATS（图标由主线程 tick 挂载）
            *NET_PREV.lock().unwrap() = Some((now, cur));
            *NET_STATS.lock().unwrap() = rows
                .into_iter()
                .take(50)
                .map(|(name, pid, text, _)| NetStat { name, pid, text })
                .collect();

            std::thread::sleep(Duration::from_secs(2));
        }
        NET_MON_ON.store(false, Ordering::SeqCst);
    });
}

/// nettop 进程键 "name.pid" → (name, pid)
fn split_key(key: &str) -> (String, u32) {
    match key.rsplit_once('.') {
        Some((n, p)) => (n.to_string(), p.parse().unwrap_or(0)),
        None => (key.to_string(), 0),
    }
}

/// 采样一轮：更新快照 / 历史 / 明细列表。
/// 主线程定时器（status_item）每 2s 调用一次。
pub fn tick() {
    let snap = SAMPLES.with(|cell| {
        let st = &mut *cell.borrow_mut();

        st.sys.refresh_memory();
        st.nets.refresh(true);

        // CPU 使用率走 host_statistics64 tick 差分（用户态/系统态拆分；
        // 总量 = 两者之和，菜单栏图标与面板折线同源）
        let (cpu_user, cpu_sys) = cpu_usage_split();
        let cpu = cpu_user + cpu_sys;
        let mem_pct = st.sys.used_memory() as f64 / st.sys.total_memory() as f64 * 100.0;

        // macOS VM 分类统计：联动 / 活跃 / 已压缩 / 实际可用
        let (mut wired_b, mut active_b, mut compressed_b, mut avail_b) = (0u64, 0u64, 0u64, 0u64);
        #[allow(deprecated)]
        unsafe {
            let port = libc::mach_host_self();
            let mut count: u32 = libc::HOST_VM_INFO64_COUNT as _;
            let mut stat = std::mem::zeroed::<libc::vm_statistics64>();
            if libc::host_statistics64(
                port,
                libc::HOST_VM_INFO64,
                &mut stat as *mut libc::vm_statistics64 as *mut _,
                &mut count,
            ) == libc::KERN_SUCCESS
            {
                let page = libc::sysconf(libc::_SC_PAGESIZE) as u64;
                wired_b = stat.wire_count as u64 * page;
                active_b = stat.active_count as u64 * page;
                compressed_b = stat.compressor_page_count as u64 * page;
                // 可用 = 总量 − 前三类（剩余量；sysinfo 的 available 公式在重压缩时饱和为 0，不适合分段展示）
                avail_b = st
                    .sys
                    .total_memory()
                    .saturating_sub(wired_b)
                    .saturating_sub(active_b)
                    .saturating_sub(compressed_b);
            }
        }

        // received()/transmitted() 是自上次刷新以来的差分字节，除以间隔即速率
        let (mut rx, mut tx) = (0u64, 0u64);
        for (name, data) in st.nets.iter() {
            if name.starts_with("lo") {
                continue; // 排除回环接口
            }
            rx += data.received();
            tx += data.transmitted();
        }

        // 磁盘读写速率：IOKit 注册表累计字节差分
        let (disk_read, disk_write) = disk_rates();

        // 明细列表：仅在对应详情视图打开时枚举（进程枚举开销远大于基础采样）
        // 先取值再 match：scrutinee 的 guard 会活到整个 match 结束，分支内再锁 = 自死锁
        let kind = ACTIVE.lock().unwrap().unwrap_or(255);
        match kind {
            0 | 1 => {
                st.sys.refresh_processes(ProcessesToUpdate::All, true);
                // CPU 使用率是两次刷新的差分：首轮无基线，数值显示 —，第二轮起为真实值
                let mut ticks = ACTIVE_TICKS.lock().unwrap();
                let warmed = *ticks >= 1;
                *ticks += 1;
                drop(ticks);
                let mut rows: Vec<(String, u32, Option<PathBuf>, f32, u64)> = st
                    .sys
                    .processes()
                    .values()
                    .filter(|p| p.name().to_string_lossy() != "statbar")
                    .map(|p| {
                        (
                            p.name().to_string_lossy().into_owned(),
                            p.pid().as_u32(),
                            p.exe().map(|e| e.to_path_buf()),
                            p.cpu_usage(),
                            p.memory(),
                        )
                    })
                    .collect();
                if kind == 0 {
                    rows.sort_by(|a, b| b.3.partial_cmp(&a.3).unwrap_or(std::cmp::Ordering::Equal));
                } else {
                    rows.sort_by(|a, b| b.4.cmp(&a.4));
                }
        let list: Vec<DetailRow> = rows
            .into_iter()
            .take(200)
            .map(|(name, pid, exe, cpu, mem)| {
                let value = if kind == 0 {
                    if warmed {
                        format!("{:.1}%", cpu)
                    } else {
                        "—".into()
                    }
                } else {
                    format!("{:.2} GB", mem as f64 / 1024.0 / 1024.0 / 1024.0)
                };
                DetailRow {
                    name: truncate(name, 30),
                    value,
                    pid: Some(pid),
                    exe,
                }
            })
            .collect();
        *DETAIL_ROWS.lock().unwrap() = list;
    }
    3 => {
        // 磁盘明细：proc_pid_rusage 累计字节差分（首轮仅记基线，不发布行）
        st.sys.refresh_processes(ProcessesToUpdate::All, true);
        let mut guard = DISK_PROC_PREV.lock().unwrap();
        let prev = guard.get_or_insert_with(HashMap::new);
        let mut cur: HashMap<u32, (u64, u64)> = HashMap::new();
        let mut rows_disk: Vec<(String, u32, Option<PathBuf>, f64, f64)> = Vec::new();
        for p in st.sys.processes().values() {
            if p.name().to_string_lossy() == "statbar" {
                continue;
            }
            let pid = p.pid().as_u32();
            unsafe {
                let mut info: libc::rusage_info_v2 = std::mem::zeroed();
                if libc::proc_pid_rusage(
                    pid as libc::c_int,
                    libc::RUSAGE_INFO_V2,
                    &mut info as *mut libc::rusage_info_v2 as *mut libc::rusage_info_t,
                ) == 0
                {
                    let (r, w) = (info.ri_diskio_bytesread, info.ri_diskio_byteswritten);
                    if let Some((pr, pw)) = prev.get(&pid) {
                        let dr = r.saturating_sub(*pr) as f64 / TICK_SECS;
                        let dw = w.saturating_sub(*pw) as f64 / TICK_SECS;
                        if dr > 0.0 || dw > 0.0 {
                            rows_disk.push((
                                truncate(p.name().to_string_lossy().into_owned(), 30),
                                pid,
                                p.exe().map(|e| e.to_path_buf()),
                                dr,
                                dw,
                            ));
                        }
                    }
                    cur.insert(pid, (r, w));
                }
            }
        }
        *prev = cur;
        rows_disk.sort_by(|a, b| {
            (b.3 + b.4)
                .partial_cmp(&(a.3 + a.4))
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        let list: Vec<DetailRow> = rows_disk
            .into_iter()
            .take(50)
            .map(|(name, pid, exe, dr, dw)| DetailRow {
                name,
                value: format!("↓{} ↑{}", fmt_speed(dr), fmt_speed(dw)),
                pid: Some(pid),
                exe,
            })
            .collect();
        *DETAIL_ROWS.lock().unwrap() = list;
    }
    2 => {
                // 网络明细：NET_STATS（后台 nettop 发布）→ 补 exe → 提取图标
                st.sys.refresh_processes(ProcessesToUpdate::All, true);
                let stats = NET_STATS.lock().unwrap().clone();
                let list: Vec<DetailRow> = stats
                    .iter()
                    .map(|st_net| {
                        let exe = st
                            .sys
                            .process(Pid::from_u32(st_net.pid))
                            .and_then(|p| p.exe().map(|e| e.to_path_buf()));
                        // 图标在渲染时经 icons::app_icon 惰性提取
                        DetailRow {
                            name: truncate(st_net.name.clone(), 30),
                            value: st_net.text.clone(),
                            pid: Some(st_net.pid),
                            exe,
                        }
                    })
                    .collect();
                *DETAIL_ROWS.lock().unwrap() = list;
            }
            _ => {
                *ACTIVE_TICKS.lock().unwrap() = 0;
            }
        }
        Snapshot {
            cpu,
            cpu_user,
            cpu_sys,
            mem_pct,
            mem_used_gb: st.sys.used_memory() as f64 / 1024.0 / 1024.0 / 1024.0,
            mem_total_gb: st.sys.total_memory() as f64 / 1024.0 / 1024.0 / 1024.0,
            wired_gb: wired_b as f64 / 1024.0 / 1024.0 / 1024.0,
            active_gb: active_b as f64 / 1024.0 / 1024.0 / 1024.0,
            compressed_gb: compressed_b as f64 / 1024.0 / 1024.0 / 1024.0,
            avail_gb: avail_b as f64 / 1024.0 / 1024.0 / 1024.0,
            rx: rx as f64 / TICK_SECS,
            tx: tx as f64 / TICK_SECS,
            disk_read,
            disk_write,
        }
    });

    *SNAPSHOT.lock().unwrap() = snap;

    {
        let mut h = HIST.lock().unwrap();
        h.cpu.push_back(snap.cpu);
        h.cpu_user.push_back(snap.cpu_user);
        h.cpu_sys.push_back(snap.cpu_sys);
        h.mem.push_back(snap.mem_pct as f32);
        h.disk_read.push_back(snap.disk_read as f32);
        h.disk_write.push_back(snap.disk_write as f32);
        if h.cpu.len() > 60 {
            h.cpu.pop_front();
            h.cpu_user.pop_front();
            h.cpu_sys.pop_front();
            h.mem.pop_front();
            h.disk_read.pop_front();
            h.disk_write.pop_front();
        }
    }
}
