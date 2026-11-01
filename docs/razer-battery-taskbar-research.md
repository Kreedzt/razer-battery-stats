# 雷蛇鼠标任务栏电量面板：调研与开发交接

- 调研日期：2026-11-01
- 文档状态：技术路线调研完成，可进入目标硬件验证与开发；尚未完成实机兼容验收。
- 产品定位：不依赖雷云的 Windows 用户态常驻电量面板，不是替换鼠标输入功能的内核驱动。
- 证据范围：开源项目说明、关键 HID 通信源码、TrafficMonitor 任务栏源码、Microsoft 官方文档。
- 验证边界：本次没有运行目标鼠标通信、编译上述项目或实测任务栏嵌入。项目声称支持与本项目实测通过必须分开记录。

## 1. 用户目标与已确认边界

### 1.1 核心目标

用户希望不再为了查看鼠标电量而等待雷云 4 慢速启动。程序应直接读取雷蛇鼠标电量，常驻显示电池图形、百分比及可用的充电状态。

### 1.2 位置要求的最终解释

用户已确认：**参考 TrafficMonitor 的任务栏显示位置即可。**

因此目标为任务栏右侧、通知区左侧的自定义面板；在常见布局中，它位于隐藏图标展开箭头左边。无需精确跟踪箭头按钮自身的矩形，也不要求箭头不存在时维持与箭头的虚构锚定关系。

普通通知区图标不等于该面板。不得未经用户同意，以普通托盘图标代替目标位置并宣称功能完成。

### 1.3 不纳入当前目标

- 完整替代雷云的 DPI、宏、灯效、配置同步或固件更新。
- 编写或安装鼠标内核过滤驱动。
- 使用 Zadig 替换鼠标输入接口驱动作为默认安装步骤。
- 首次交付即保证所有雷蛇型号、连接方式和 Windows 版本兼容。
- 云服务、账户系统、远程遥测、自动更新等与电量显示无关的功能。

## 2. 调研是否充分

**对于选型和制定可执行开发计划，已经充分；对于承诺具体设备兼容，仍需实机验证。**

已明确的问题：

1. Windows 用户态 HID 有可用的开源直读实现，无需依赖雷云日志。
2. 电量、充电命令和常见型号差异已有源码依据，不需要从零猜测协议。
3. TrafficMonitor 的位置实现有源码参考，可以作为任务栏面板的技术路线。
4. 任务栏嵌入不是 Windows 官方承诺稳定的面板扩展契约，需按系统版本维护。
5. 已识别代码复用许可证、型号支持声明、睡眠误判和协议校验等风险。

继续泛搜更多同类托盘工具的收益较低。下一步应将不确定性转为目标设备和目标 Windows 环境的运行证据。

## 3. 开源项目评估

| 项目 | 技术与数据来源 | 雷云依赖 | 适用价值 | 已知边界 |
| --- | --- | --- | --- | --- |
| OpenRazer | Linux 内核 HID 驱动、守护进程、Python 接口 | 无 | 协议、PID、transaction、充电能力的主要参考 | 不是可直接安装在 Windows 的驱动；Linux 支持不等于 Windows 已实测 |
| xzeldon/razer-battery-report | Rust、hidapi、独立 razer-hid 库 | 无 | 首选评估的采集库；提供 CLI 诊断和完整应用参考 | 型号表来自 OpenRazer；当前 UI 为普通托盘，不能直接满足面板位置 |
| nuxencs/razertray | Rust、hidapi windows-native | 无 | 原生 HID 传输、协议验证、型号参数表、数字托盘显示 | 蓝牙未测试；接口选择及充电错误处理仍有边界 |
| Fan4Metal/mouse_tray | Python、hidapi | 无 | 简洁协议示例、多厂商电量 UI | 雷蛇仅明确实测 Viper V2 Pro；不查询充电；没有确认明确许可证 |
| Tekk-Know/RazerBatteryTaskbar | Electron、node-usb、原始 USB 控制传输 | 无直接日志依赖 | 早期实现参考 | 请求构造和型号表存在静态问题；Windows USB 后端可能需额外配置 |
| sanraith/razer-taskbar | Electron/TypeScript、读取雷云日志 | 必须运行雷云 3/4 | 仅参考显示交互 | 已归档；不能实现真正脱离雷云 |
| TrafficMonitor | C++、Win32 自定义任务栏窗口 | 与雷云无关 | 目标显示位置、DPI 和任务栏布局处理 | 非官方嵌入方案；不是雷蛇电量后端 |

### 3.1 优先评估：razer-battery-report / razer-hid

关键实现：

- `crates/razer-hid/src/transport.rs`：HID feature report 读写、长度检查。
- `crates/razer-hid/src/report.rs`：90 字节协议主体、编码解码、XOR 校验、命令定义。
- `crates/razer-hid/src/device.rs`：电量与充电查询、忙碌和无响应处理、命令匹配。
- `crates/razer-hid/src/catalog.rs`：型号参数。
- `crates/razer-hid/src/discovery.rs`：设备发现。

已读源码确认其区分充电不支持与未充电，并有多接口和设备去重设计。它不是所有兼容性问题均已解决的保证：例如，进程内互斥不能防止雷云或另一个程序跨进程并发访问。

该库文档说明尚未发布到 crates.io，可通过 Git 依赖使用。开发时必须固定经过核查的 tag/commit，不直接依赖浮动 HEAD。

现成应用提供如下诊断入口，适合硬件可行性验证：

```powershell
.\razer-battery-report.exe status --json
.\razer-battery-report.exe diagnose
```

以上是待执行命令，不是本次运行结果。使用时以下载版本的实际文件名与 CLI 帮助为准。

### 3.2 备选参考：razertray

已读实现使用 hidapi 的 Windows 原生后端，直接发送和读取 feature report，不走默认的 Zadig/WinUSB 替换路线。

可参考的内容：

- 按型号选择 transaction 和充电能力。
- 校验报告长度、CRC、命令与状态。
- 默认 60 秒轮询，说明这类应用无需高频电量读取。

需注意：

- 蓝牙未测试。
- 某些充电查询失败被转为未充电；本项目应保留“未知”。
- scanner 的接口选择不等于所有设备接口均已验证。
- 响应对应关系检查未覆盖所有可能字段；不能直接宣传为完整协议审计通过。

### 3.3 不建议直接移植的实现

`mouse_tray` 的雷蛇代码硬编码 `0x1F`，充电状态写死为 false，并以百分比为零判断睡眠。零电量本身合法，不能用它证明设备睡眠。代码中的协议字段注释也存在错位，但实际请求头中的电量命令仍是 `07/80`。

`RazerBatteryTaskbar` 的静态审阅发现：transaction 参数未正确从产品表接入请求对象、VID 过滤表达式不正确、若干型号 transaction/PID 名称与 OpenRazer 不一致，以及缺少完整响应校验。未运行复现这些路径，不将静态推断描述为实测故障。

## 4. Windows 通信与协议依据

### 4.1 用户态访问路线

Windows 官方提供 HID collection 枚举、句柄访问和 feature report API。可以使用 hidapi，也可以直接封装 SetupAPI、CreateFile、HidD_SetFeature 和 HidD_GetFeature。

推荐先沿 Windows 原生 HID 路径验证，不安装额外内核驱动。正确的 HID collection、访问权限及 report 长度必须由实际设备描述和运行结果确认，不能只按 VID/PID 找到第一个接口就认为成功。

### 4.2 典型协议布局

以下偏移针对不含 HID report ID 的 90 字节 Razer 主体：

| 偏移 | 字段 | 说明 |
| --- | --- | --- |
| 0 | status | 请求通常为 00；成功响应为 02 |
| 1 | transaction ID | 按型号选择，常见 1F、3F、FF |
| 2–3 | remaining packets | 大端；普通单包请求为 0000 |
| 4 | protocol type | 当前查询使用 00 |
| 5 | data size | 电量与充电查询均为 02 |
| 6 | command class | 07 |
| 7 | command ID | 电量 80；充电 84 |
| 8–87 | arguments | 读数位于 arguments[1]，即主体偏移 9 |
| 88 | checksum | 主体偏移 2–87 逐字节 XOR |
| 89 | reserved | 00 |

电量请求的前八字节：

```text
00 TX 00 00 00 02 07 80
```

电量原始值为 0–255，按 `raw * 100 / 255` 换算百分比；具体取整方式应保持一致。

HIDAPI 路径通常在主体前增加 report ID 0，因此缓冲为 91 字节，读数偏移变为 10。直接 USB 控制传输与 HIDAPI 的偏移不能混用。Windows 原生实现还应遵循 HIDP_CAPS 的 FeatureReportByteLength，不能不检查设备能力就盲目发送固定长度。

### 4.3 型号和连接方式差异

OpenRazer 电量查询中的典型配置：

| 型号示例 | transaction |
| --- | --- |
| Viper V2 Pro、DeathAdder V3 Pro、Basilisk Ultimate | 1F |
| DeathAdder V2 Pro、Mamba Wireless、早期 Lancehead | 3F |
| Viper Ultimate、Basilisk X HyperSpeed | FF |

配置应绑定具体 PID/连接路径，不能仅绑定营销名称。同一鼠标的有线、接收器和高轮询率接收器可能不同；蓝牙需独立验证。部分型号不支持充电查询，不能一律发送并把失败当作未充电。

### 4.4 通信安全边界

- 只发送目标设备已确认的状态查询，不修改 DPI、配对、灯光或固件。
- 检查报告长度、校验、成功状态及与请求的对应关系；特殊 transaction 回显行为按型号证据处理。
- 单个设备的请求与响应串行处理，避免本程序读取自己的另一条查询响应。
- 超时、忙碌、断开和不支持命令不能变成 0% 电量。
- 所有 HID 操作离开 UI 线程。具体后端的阻塞与取消行为需验证，不能认为逻辑重试次数就等于系统调用有截止时间。
- 若雷云同时运行，应单独验证是否争用；不得声称进程内锁能协调雷云。

## 5. 任务栏显示路线

### 5.1 为什么不是 Shell_NotifyIcon

Shell_NotifyIcon 支持通知区图标、提示和回调，但没有任意坐标或面板宽度参数。可见性和排序由 Shell 与用户管理。它适合辅助入口，不足以满足本项目的主显示位置。

### 5.2 TrafficMonitor 的实际方式

公共流程：查找任务栏窗口、创建自身显示窗口、用 SetParent 挂接，再按任务栏内部几何布局定位。

经典任务栏：

- 查找 ReBarWindow32 / WorkerW、MSTaskSwWClass / MSTaskListWClass 等内部窗口。
- 缩小应用按钮区域，在右侧留出面板宽度。
- 退出时恢复布局。

Windows 11：

- 查找任务栏下的 TrayNotifyWnd 等内部窗口。
- 以通知区左边界减去面板宽度定位。
- 已有针对副屏、组件、偏移和部分系统 build 的适配。
- 此路径没有经典实现中同样的应用按钮区域缩宽操作，不能假设一定获得布局保留空间。

这些内部窗口结构不是微软提供的稳定第三方面板协议。公开 API 可用不代表这种组合用途享有跨版本兼容承诺。

### 5.3 采用方式

**推荐：独立应用参考 TrafficMonitor 的位置与布局思路，不要求用户安装 TrafficMonitor。**

建议技术栈为 Rust + hidapi/razer-hid + Win32。选择 Rust 的主要原因是已有可评估的 Rust HID 库；若团队更熟悉 C++，C++ + hidapi + Win32 也是可行替代，不必为了语言而跨语言封装整个应用。

TrafficMonitor 插件是备选而非既定交付：插件接口支持文字、自绘、DPI 宽度和鼠标事件，可减少任务栏适配工作，但会引入宿主依赖。用户目前仅确认参考其位置，没有要求依赖该程序。

DeskBand 不作为新项目主路线：虽有历史接口文档，Microsoft 已明确不推荐新开发采用，不能作为 Win10/11 通用支持承诺。

### 5.4 必须处理的显示生命周期

- Explorer 重启后旧 HWND 失效，重新发现与挂接。
- DPI、主题、分辨率、任务栏尺寸及显示器变化。
- 自动隐藏和全屏窗口下不残留置顶条。
- 面板不遮挡应用按钮，不抢夺任务栏其他区域的点击。
- 退出或挂接失败时恢复自身改动；恢复前重新确认句柄有效及当前布局，不能写回过期几何数据。
- 跨进程 SetParent 的 DPI awareness 行为按 Microsoft 文档和实际系统验证。

嵌入失败必须明确报告，不得静默改成悬浮窗后仍标记“任务栏面板已实现”。可提供用户主动选择的其他显示方式，但不改变验收条件。

## 6. 建议架构与状态模型

采用单个普通用户进程，不建立 Windows 服务。至少分离以下职责，但不要求为了模块边界拆成多个 crate：

1. 设备发现与型号参数：识别受支持 PID、连接方式和 HID collection。
2. 电量采集：串行查询，生成带时间戳的读数。
3. 状态存储：保存当前有效状态和最后一次有效读数。
4. 任务栏宿主：挂接、定位、恢复、DPI 与主题处理。
5. 面板绘制：只读状态，不在绘制回调中执行设备 I/O。

建议状态字段：

| 字段 | 语义 |
| --- | --- |
| device_key | 用于选择及去重的设备身份；不能假设所有型号都能读到序列号 |
| connection | 有线、接收器、蓝牙或尚未确认 |
| availability | 可读取、暂时无响应、已断开、协议不支持或错误 |
| level_percent | 当前有效读数，可为空；零是合法值 |
| charging | 正在充电、未充电、未知、不支持 |
| last_valid_reading | 最后有效电量与充电信息 |
| last_success_at | 最后成功时间，用于标记过期 |

界面语义：

- 有效读数：电池轮廓与百分比。
- 充电：仅在设备明确报告时显示充电标记。
- 暂时无响应：保留上次读数但显式标记过期，提示最后更新时间。
- 已断开：显示断开或不可用，不显示 0%。
- 未知状态：明确未知，不推导为未充电。

不凭一次超时断言设备睡眠；睡眠、关机和链路故障都可能无响应。不根据电量百分比直接生成“还能使用几小时”，除非未来有可靠估算依据且用户要求。

## 7. 轮询与轻量性

“实时”定义为后台自动更新，不需要启动雷云或手动刷新；不代表毫秒级采样。

建议初始参数，均待实机测量：

- 正常使用：30–60 秒查询一次。
- 充电：可缩短到 10–15 秒。
- 打开详情：请求一次新读数，合并正在执行的查询。
- 热插拔、系统恢复：重新发现并刷新。
- 无响应：降低重复访问频率；不通过持续高频查询阻止设备休眠。
- 只在状态变化或界面需要时重绘。

这些数值不是已验证的最优配置。需记录实际请求耗时、空闲 CPU、工作集、首次有效读数时间，以及轮询对鼠标休眠的影响。不预先承诺具体内存、CPU 或启动时间指标。

## 8. 许可证与供应链边界

| 来源 | 本次查到的授权信息 | 使用要求 |
| --- | --- | --- |
| OpenRazer 关键驱动文件 | GPL-2.0-or-later | 区分研究协议与复制代码；复制时评估适用义务 |
| razertray | Cargo.toml 声明 GPL-2.0-or-later | 按具体版本核实完整许可后复用 |
| razer-battery-report / razer-hid | 项目声明 MIT | 核实选定版本、保留声明并检查上游来源与依赖 |
| TrafficMonitor | Anti-996 License 1.0 Draft | 不能误标为 MIT/GPL；复制时遵循原文条款 |
| mouse_tray | API license 为 null，未确认独立许可 | 不将公开仓库等同于获得任意再分发授权 |
| RazerBatteryTaskbar | package.json 声明 ISC，未见独立 LICENSE | 上游来源和授权需另行核实，不作为首选底座 |
| sanraith/razer-taskbar | MIT | 技术上仍不符合无雷云依赖目标 |

本节不是法律意见，也不是完整依赖审计。进入开发时固定具体源码版本，登记复制文件与来源；公开发布前再检查最终组合。不要因为借鉴了多个项目就自动给全部代码统一贴上 MIT。

## 9. 实施 checkpoint 与退出条件

以下为建议顺序，不表示任何 checkpoint 已完成。

### CP0：确定运行基线

- [ ] 枚举实际设备，记录型号、VID/PID、接口、连接方式、接收器信息。
- [ ] 记录 Windows 版本/build、任务栏实现、DPI、显示器布局及第三方任务栏修改工具。
- [ ] 从工具可获取的信息先自动采集，不要求用户手工抄写可枚举数据。
- [ ] 对工具无法确认的鼠标营销型号、接收器型号再向用户确认。
- [ ] 固定待评估开源版本并记录许可证。

退出条件：可以精确描述“本次要验证的硬件与系统组合”，而不是笼统的“雷蛇鼠标”。

### CP1：证明无需雷云读取电量

- [ ] 先使用现成 CLI 获取实际电量，保存必要输出。
- [ ] 经用户允许退出雷云及相关后台后，重复读取；不要擅自卸载或永久禁用其服务。
- [ ] 普通用户权限、保留原有鼠标输入驱动。
- [ ] 确认电量与充电状态正确；需要比对时可在另一时刻查看雷云，避免把比对阶段与独立运行阶段混淆。
- [ ] 验证休眠、关闭、拔插、有线/无线切换及无线接收器仍连接但鼠标关闭的情况。
- [ ] 不支持的路径明确记为未支持，不伪造读数。

退出条件：目标设备的真实读数和故障状态已有可复现证据。未通过前，不以设备库存在为由宣称硬件功能已完成。

### CP2：证明任务栏位置与生命周期

- [ ] 在目标系统创建实际可见面板，位于通知区左侧。
- [ ] 验证按钮拥挤、自动隐藏、全屏、DPI 和主题变化。
- [ ] 验证 Explorer 重启后恢复。
- [ ] 验证退出恢复布局、无残留和无错误点击区域。
- [ ] 记录截图或录屏、系统 build 与缩放。

退出条件：实际桌面上的目标位置可用；仅计算坐标、编译通过或普通托盘截图不算完成。

### CP3：接入真实状态并完成交互

- [ ] HID 工作线程与 UI 分离，界面不因慢响应卡顿。
- [ ] 显示百分比、电池轮廓及可靠的充电标记。
- [ ] 状态过期、未知和断开有明确表示。
- [ ] 配置的目标设备不会在断开后静默切换为另一只鼠标。
- [ ] 若同时发现同一鼠标的有线与接收器路径，按可验证身份去重。
- [ ] 提供必要的退出、刷新和设备信息入口。

退出条件：目标鼠标到目标任务栏位置的端到端路径可用，不依赖雷云。

### CP4：发布验收

- [ ] 普通用户运行，无额外鼠标驱动安装步骤。
- [ ] 实测启动到首个有效读数时间、空闲 CPU、内存及轮询耗时。
- [ ] 测试持续运行、系统睡眠恢复及任务栏反复重建。
- [ ] 若提供开机启动，确保可关闭且卸载/移除时无残留入口。
- [ ] 验证分发产物包含必要依赖、版权和许可证。
- [ ] 列出实际验证的硬件与系统组合；未知型号不标记为已实测支持。

退出条件：可交付的真实运行程序与证据齐备；不得以空壳、模拟电量或仅单元测试通过替代。

## 10. 验证矩阵

| 维度 | 必须观察的结果 |
| --- | --- |
| 雷云完全退出 | 仍可取得新读数，而非旧日志或缓存 |
| HID 权限 | 普通用户可访问正确 collection，不影响鼠标输入 |
| 电量边界 | 0% 合法；无响应和坏包不显示为 0% |
| 充电状态 | 支持时正确显示；不支持/失败不假装未充电 |
| 鼠标关闭或休眠 | 不阻塞 UI，旧值有过期标记 |
| 接收器拔插 | 识别断开并在重连后恢复 |
| 有线与无线并存 | 不重复显示同一物理设备，身份不足时不盲目合并 |
| 雷云并发运行 | 单独测试是否争用；未测就明确未测 |
| 任务栏按钮拥挤 | 不遮挡、不改变其他按钮点击行为 |
| Explorer 重启 | 重新挂接，旧句柄不被继续使用 |
| DPI/主题/多屏变化 | 目标环境内位置、尺寸和可读性正确 |
| 自动隐藏/全屏 | 无悬浮残留或抢前台 |
| 程序退出 | 释放句柄、移除自身窗口、恢复所改布局 |

永久测试优先覆盖协议响应检查、合法零电量、过期状态、设备身份与状态迁移。任务栏位置必须通过实际桌面观察；模拟 HWND 或截图之外的纯单元测试不能证明嵌入兼容。

## 11. 尚未解决但已明确的事项

1. 目标鼠标准确型号与连接路径尚未确定。
2. 目标设备在无雷云时的 HID 访问尚未实测。
3. 目标 Windows 任务栏挂接、布局与恢复尚未实测。
4. 拟复用库与最终分发物的固定版本、许可证组合尚未确定。
5. 对额外 Windows build、蓝牙及其他型号的支持范围，需要根据实际验收确定，不从设备表直接推导。

这些事项不妨碍开始开发，但决定了何时可以承诺兼容性。当前不需要继续无边界扩充调研项目列表。

## 12. 一手资料索引

### Windows 官方接口

- [Opening HID Collections](https://learn.microsoft.com/en-us/windows-hardware/drivers/hid/opening-hid-collections)
- [HidD_GetFeature](https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/hidsdi/nf-hidsdi-hidd_getfeature)
- [Notifications and the Notification Area](https://learn.microsoft.com/en-us/windows/win32/shell/notification-area)
- [The Taskbar：含 TaskbarCreated 生命周期](https://learn.microsoft.com/en-us/windows/win32/shell/taskbar)
- [SetParent：含 DPI awareness 注意事项](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setparent)
- [IDeskBand：官方支持状态](https://learn.microsoft.com/en-us/windows/win32/api/shobjidl_core/nn-shobjidl_core-ideskband)

### 电量协议及 Windows 实现

- [OpenRazer 项目](https://github.com/openrazer/openrazer)
- [OpenRazer 协议结构](https://github.com/openrazer/openrazer/blob/master/driver/razercommon.h)
- [OpenRazer 命令构造](https://github.com/openrazer/openrazer/blob/master/driver/razerchromacommon.c)
- [OpenRazer 鼠标型号与电量查询](https://github.com/openrazer/openrazer/blob/master/driver/razermouse_driver.c)
- [razer-battery-report](https://github.com/xzeldon/razer-battery-report)
- [razer-hid 库说明](https://github.com/xzeldon/razer-battery-report/blob/HEAD/crates/razer-hid/README.md)
- [razer-hid 协议](https://github.com/xzeldon/razer-battery-report/blob/HEAD/crates/razer-hid/src/report.rs)
- [razer-hid 传输](https://github.com/xzeldon/razer-battery-report/blob/HEAD/crates/razer-hid/src/transport.rs)
- [razer-hid 查询及状态处理](https://github.com/xzeldon/razer-battery-report/blob/HEAD/crates/razer-hid/src/device.rs)
- [razertray 已检查提交的通信实现](https://github.com/nuxencs/razertray/blob/39a2a60f8d477a60733cba69c47befe583325cf0/src/hid/client.rs)
- [razertray 已检查提交的依赖与许可声明](https://github.com/nuxencs/razertray/blob/39a2a60f8d477a60733cba69c47befe583325cf0/Cargo.toml)
- [mouse_tray 雷蛇实现](https://github.com/Fan4Metal/mouse_tray/blob/master/mouse_tray/drivers/vendor/razer.py)
- [RazerBatteryTaskbar 已检查提交的主逻辑](https://github.com/Tekk-Know/RazerBatteryTaskbar/blob/05132f704304edd45ba6bacb6692158765af821e/src/main.js)
- [node-usb Windows 后端说明](https://github.com/node-usb/node-usb#windows)
- [依赖雷云日志的 razer-taskbar](https://github.com/sanraith/razer-taskbar)

### 任务栏显示

- [TrafficMonitor 公共任务栏窗口](https://github.com/zhongyang219/TrafficMonitor/blob/master/TrafficMonitor/TaskBarDlg.cpp)
- [TrafficMonitor 经典任务栏布局](https://github.com/zhongyang219/TrafficMonitor/blob/master/TrafficMonitor/ClassicalTaskbarDlg.cpp)
- [TrafficMonitor Windows 11 布局](https://github.com/zhongyang219/TrafficMonitor/blob/master/TrafficMonitor/Win11TaskbarDlg.cpp)
- [TrafficMonitor 插件接口](https://github.com/zhongyang219/TrafficMonitor/blob/master/include/PluginInterface.h)
- [TrafficMonitor 许可证](https://github.com/zhongyang219/TrafficMonitor/blob/master/LICENSE)

说明：HEAD/master 链接会变化。它们用于导航，本次对话中的读取结果是调研证据；实际开发应重新核查并固定具体提交，不能假设未来分支内容与本次相同。
