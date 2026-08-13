# ADR-0002：多供应商额度与 Windows 凭据存储

- 状态：Accepted
- 日期：2026-08-14
- 适用版本：QuotaDock 0.6.1+

## 背景

QuotaDock 原先只有 Codex 1 周额度，一个全局快照与刷新锁无法准确表达多个数据源的
配置、部分成功、独立旧值和独立故障。DeepSeek 余额与 Kimi Coding Plan 查询需要 Bearer API Key，
把 Key 放入普通 JSON、前端 store 或日志会扩大泄漏面。Moonshot API 开放平台和 Kimi
Coding Plan 是独立产品，不能合并余额、额度或凭据口径。

## 决策

1. schema v6 以三个 `ProviderState` 为事实源，每个供应商独立保存配置状态、最新成功
   快照、最后尝试、健康与稳定错误分类。旧全局 Codex 字段只作迁移/兼容投影。
2. 刷新协调器按供应商防重入并并行查询；每个完成结果立即短持锁原子提交、递增 revision
   并广播。失败只保留本供应商旧值，不能回滚其他成功结果。
3. Codex 沿用 App Server 主源与 PTY 降级。DeepSeek 和 Kimi 适配器只调用固定官方
   HTTPS endpoint，禁用重定向并限制超时与响应体。
4. v0.6.1 的 Kimi 范围是 Coding Plan：使用官方 Kimi Code 开源客户端同源的固定
   `https://api.kimi.com/coding/v1/usages`，保存总量及全部时间窗口。官方接口未保证
   Kimi/Code 顶部用量分段，因此不抓网页、不臆造分项。
5. DeepSeek/Kimi Key 由 Rust 后端写入 Windows Credential Manager，固定 service 为
   `com.rupingliu.quotadock`，账户分别为 `deepseek-api-key`、`kimi-code-api-key`。
   前端只获得配置状态，不能读取 secret。
6. 普通状态 JSON 以本地明文保存额度/余额快照和设置，但不保存认证凭据或 API Key。
   Bearer header、上游错误正文、完整响应、PTY 原文与 secret 不进入持久状态、诊断、
   日志、事件或测试快照。额度、余额和趋势仍应按本机财务数据保护。
7. 删除连接与删除普通应用数据保持为两个明确动作。卸载器不隐式删除用户凭据；用户应
   在卸载前通过详情页删除，或在 Windows 凭据管理器中按固定 service 清理。
8. 悬浮条仍为 `260 × 36px`，只轮播用户勾选且已配置的供应商；默认仅 Codex。

## 安全边界

- 接受用户输入 Key 短暂经过本地 Tauri IPC 的桌面应用威胁模型；不接受网页远程读取 Key。
- 接受官方额度可能延迟；不宣称实时，不从差额推断交易、套餐或未公开的产品分项。
- 系统代理可影响传输路径，但 TLS 主机验证、固定 HTTPS host、禁重定向仍必须生效。
- 状态恢复会原样备份损坏或外部污染文件。若外部已写入敏感内容，备份可能保留它；
  这不等同于 QuotaDock 生成或保存供应商 Key。
- Windows Credential Manager 的访问控制与生命周期属于操作系统边界。当前未承诺
  macOS/Linux 凭据后端，也不承诺卸载自动清理。

## 结果

- 单家慢、失败或未配置不会阻塞或污染其他供应商。
- Key 的持久化与普通余额状态分离，前端与诊断面不具备回读能力。
- 严格固定端点缩小 SSRF、重定向泄露和私有接口漂移风险。
- 多供应商状态与 revision 增加实现和迁移复杂度；必须维持 Rust/TypeScript DTO 镜像及
  乱序事件测试。

## 被否决方案

- 把 Key 写入设置 JSON或环境文件：明文生命周期和备份边界不可控。
- 抓取控制台 Cookie/DOM：依赖私有界面并扩大账号凭据权限。
- 允许任意 base URL：可能把 Bearer Key 发送到非官方主机。
- 等三家全部成功后一次提交：慢供应商会延迟快结果，单家失败会破坏部分成功。
- 把 Moonshot API 余额当成 Kimi Coding Plan：两个产品的 Key、计费和额度完全不同。
