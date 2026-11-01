# Task: 雷蛇电量任务栏面板（CP1→CP3）

- Risk: 中 —— 跨 HID 协议与 Win32 壳层嵌入；对外部副作用仅只读查询；新仓库逐 checkpoint 可回滚
- Commit policy: auto（功能分支，每 checkpoint 单一语义 commit；push 单独确认）
- Branch: feature/razer-battery-taskbar

## Goal

Win10 经典任务栏（通知区左侧）常驻面板，脱离雷云读取 Razer DeathAdder V3 Pro 电量并显示百分比、电池轮廓与可靠的充电状态。

## Scope

- 必须完成：
  - CP1 用 razer-battery-report v1.3.0 CLI 证明无雷云可读电量（含故障路径）
  - CP2 自研 Rust 程序在经典任务栏嵌入面板并管理生命周期
  - CP3 接入 razer-hid（git tag v1.3.0）轮询真实电量，完成状态显示与交互
- 明确不做：
  - CP4 发布验收（性能实测、分发、开机启动、其他型号/蓝牙）
  - DPI/宏/灯效/配置同步；内核驱动；Zadig/WinUSB
  - Win11 任务栏路线（本机 19045 为经典任务栏）
  - 电量→可用时长估算

## Decisions

- 技术栈 Rust + razer-hid(git tag v1.3.0) + Win32 —— 调研推荐且本机 cargo 就绪（2026-11-01）
- 验证期允许结束雷云进程（RazerAppEngine 等），不卸载不禁用 —— 用户授权（2026-11-01）
- 接收器可用于无线路径验证 —— 用户确认（2026-11-01）
- 当前连接：无线，接收器呈现 PID 00B7（= DAV3 Pro Wireless；有线为 00B6）—— razer-hid catalog v1.3.0 判定（2026-11-01）

## Acceptance

- 成功路径：
  - A1 雷云进程结束后 CLI `status --json` 返回非零电量 + 充电状态（保存输出为证据）
  - B1 面板出现在任务栏通知区左侧，不遮挡任务栏按钮，鼠标点击穿透到该穿透的区域
  - C1 自研程序显示百分比与电池轮廓，30–60s 轮询，充电仅在设备明确报告时显示
  - C2 面板右键菜单提供退出；退出后任务栏布局恢复、无残留
  - C3 Explorer 重启后面板自动重新挂接并恢复读数
- 失败/边界路径：
  - F1 鼠标关闭/休眠：读取超时/失败被明确报告，UI 保留旧值 + 过期标记，绝不显示 0%
  - F2 拔接收器：进入断开状态；重插后自动恢复读取（热插拔事件）
  - F3 充电查询失败：显示未知，不显示未充电
  - F4 嵌入失败：明确报告错误，不静默降级为悬浮窗
- 不允许发生：
  - 向设备发送电量/充电以外的写命令
  - 普通用户权限之外的要求（驱动安装、管理员）
  - 雷云被卸载或服务被禁用/删除
  - UI 线程执行 HID I/O 或长阻塞

## Evidence

- 自动检查：`cargo build --release`、`cargo clippy -- -D warnings`、`cargo test`
- CP1 证据：`tools/razer-battery-report-v1.3.0-x86_64-pc-windows-msvc.exe status --json`（雷云退出前后各一次，输出存 `docs/tasks/.../evidence/`）
- 人工检查：面板位置/遮挡/DPI/Explorer 重启/退出恢复 —— 用户提供截图或目视确认
- 高风险 diff：无资金/生产风险；HID 请求构造（`src/hid/`）与窗口挂接/恢复（`src/shell/`）为重点阅读区

## Checkpoints

- [ ] 0. 基线与仓库初始化（已采集：硬件 PID/接口、OS build、双屏、雷云进程；建分支、清根目录重复调研文件、固定工具版本）
      证据：枚举输出已记录 ｜ commit：
- [ ] 1. CP1 CLI 硬件验证：下载 v1.3.0 portable → 雷云运行时读一次 → 结束雷云进程再读 → 鼠标关机读 → 拔插接收器 → （可选）有线 00B6 读
      证据：JSON 输出 + 雷云进程清单前后对比 ｜ commit：
- [ ] 2. CP2a Rust 项目骨架 + 任务栏嵌入：经典任务栏定位（ReBar/MSTaskSwWClass 路线）、SetParent、缩宽留位、DPI
      证据：cargo build + 用户截图（通知区左侧可见面板）｜ commit：
- [ ] 3. CP2b 生命周期：TaskbarCreated 重挂接、退出恢复布局、无残留、自动隐藏/全屏不抢前台
      证据：用户操作确认（重启 Explorer、退出程序）｜ commit：
- [ ] 4. CP3a HID 集成：razer-hid 后台线程轮询 → 状态模型（valid/stale/disconnected/unknown）→ 面板显示
      证据：真实读数截图 + F1/F2/F3 行为 ｜ commit：
- [ ] 5. CP3b 交互与打磨：右键退出/刷新、充电标记、过期提示、双链路去重（若同现）
      证据：用户验收 + clippy/test 通过 ｜ commit：

## Pitfalls

（出现时按 §Pitfall 追加）

## Sources

- 调研：docs/razer-battery-taskbar-research.md
- razer-hid catalog（PID→tid/接口/充电）：razer-battery-report tag v1.3.0 crates/razer-hid/src/catalog.rs
- TrafficMonitor 经典任务栏嵌入参考：TrafficMonitor/ClassicalTaskbarDlg.cpp
- Windows：SetParent DPI 注意事项、TaskbarCreated 消息
