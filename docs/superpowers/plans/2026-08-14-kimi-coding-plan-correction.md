# Kimi Coding Plan 修正与 v0.6.1 发布计划

## 已确认口径

- Kimi 指 Kimi Coding Plan / Kimi Code 套餐，不是 Moonshot API 开放平台余额。
- 官方用量包含套餐总量和 Code 的 5 小时、7 天等窗口；总量可能受 Kimi 会员月总额度约束。
- 官方公开客户端的稳定接口只保证 `usage`、`limits` 与可选加油包字段，没有保证网页中
  Kimi/Code 彩色分段的结构，因此不抓 DOM、不调用未文档化私有接口、不伪造拆分值。
- DeepSeek 仍显示充值余额；悬浮文字可直接打开固定官方充值页面。

## 实施

1. schema v6 将 Kimi 快照改为总量与任意时间窗口；used/limit 保存为整数字符串。
2. 使用固定 `https://api.kimi.com/coding/v1/usages` 与独立 `kimi-code-api-key` 凭据。
3. v5→v6 只清除错误 Kimi API 余额，保留 Codex、DeepSeek、趋势和设置。
4. `260 × 36px` 中显示 `总 xx%` 与剩余最少的一个窗口；详情页展示全部窗口和重置时间。
5. 轮播定时器只随选择集合、暂停状态和显式手动切换重启，不随 revision/状态刷新重启。
6. DeepSeek“充值余额”成为可聚焦按钮，停止拖动事件并打开固定官方充值页。
7. 完成前端、Rust、迁移、安全与发布构建门禁后，签名发布 v0.6.1。

## 验收

- 8 秒轮播在持续状态更新下仍会前进；手动、悬停、焦点、隐藏与 reduced-motion 行为不回归。
- Kimi fixtures 覆盖总量、5 小时、7 天、大整数、缺失总量、非法数值和 HTTP 错误。
- DeepSeek 充值按钮鼠标/键盘可用，不触发窗口拖动，只能打开固定 HTTPS 页面。
- v5 迁移保留 Codex/DeepSeek，清除旧 Kimi API 余额并移出轮播。
- 所有自动化门禁与签名安装包验证通过后才发布；真实账户契约只在维护者明确提供临时 Key 时执行。
