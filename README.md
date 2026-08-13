# QuotaDock

QuotaDock 是一个非官方、Windows 优先的本地额度与余额状态工具。它在保持
`260 × 36px` 的常驻悬浮条中，按用户选择循环显示 Codex 1 周额度、DeepSeek API
充值余额和 Kimi API 可用余额；详情页提供三家完整状态、独立刷新和连接管理。

当前版本：`0.6.0`

## 当前能力

- Codex：优先通过 App Server 结构化接口读取 1 周额度；PTY `/status` 仅作兼容降级。
- DeepSeek：通过官方 API 开放平台余额接口读取总额、赠金和充值余额，支持官方返回的多币种。
- Kimi：仅支持国内 API 开放平台，读取人民币可用、现金和代金券余额；它与 Kimi 会员、Kimi Code 不互通。
- 三家独立刷新、独立错误与旧值保留；“刷新全部”允许部分成功。
- 悬浮条默认只显示 Codex。详情页可选择参与项，固定按 Codex → DeepSeek → Kimi
  每 8 秒循环；只有一项时不计时，hover、键盘焦点或页面隐藏时暂停。
- 可点击/聚焦供应商标签手动切换；系统开启“减少动态效果”时只保留手动切换。
- 系统托盘提供刷新全部、显示/隐藏、开机启动、更新检查和退出。
- 启动后及每 6 小时检查 GitHub Release；安装包须通过应用内 Ed25519 签名与版本校验。

余额可能受上游结算或同步延迟影响，DeepSeek/Kimi 官方控制台以及 Codex 官方用量页
仍是最终核验来源。QuotaDock 不执行充值、支付、退款，也不抓取控制台网页或私有接口。

## 配置与隐私

DeepSeek 和 Kimi 国内站的 API Key 由用户在详情页主动输入。Rust 后端使用固定服务名
`com.rupingliu.quotadock` 将它们保存到当前 Windows 用户的 Credential Manager；前端只会
得到“已配置/未配置/不可用”，不会读回密钥。Key 不写入普通状态 JSON、诊断、
探针日志或更新数据。

Tauri 应用数据目录的 `quotadock-state.json` 使用 schema v5，以本地明文保存最新额度/
余额快照、Codex 7 天稀疏趋势、轮播选择、设置与恢复提示，但不保存认证凭据或 API Key。
请将余额和趋势按本机财务数据保护。Codex 原始 CLI 输出不会持久化；发布构建不启用
可执行命令覆盖或探针落盘，测试构建的探针日志也只记录脱敏摘要。损坏或不兼容的状态
文件会原样备份并只保留最近 3 份；若用户或外部程序已把敏感内容写入该文件，恢复备份
也可能原样包含这些内容。

“删除连接”只删除对应的 Windows 凭据并将该供应商移出轮播，不删除普通状态文件；
删除普通应用数据也不会删除 Windows 凭据。这两项必须分别操作。当前安装器卸载时不会
主动清理用户数据或 Credential Manager 条目：建议在卸载前从详情页删除连接，已卸载时
可在 Windows 凭据管理器中删除上述服务名下的条目。

更新器只访问本项目 GitHub Release，并用内置 Ed25519 公钥验证签名，同时核对安装包
ProductVersion 和本机最高可信版本。Windows 安装包尚未配置商业 Authenticode 证书，
SmartScreen 仍可能显示“未知发布者”；这与应用内更新签名是两条独立信任链。

## 开发与验证

需要 Node.js、Rust、WebView2 与 MSYS2 UCRT64 工具链。安装依赖并运行：

```powershell
npm install
npm run dev
npm run tauri dev
```

发布门禁：

```powershell
npm test
npm run build
cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
cargo check --manifest-path src-tauri/Cargo.toml --all-targets --all-features
cargo test --manifest-path src-tauri/Cargo.toml --all-features
git diff --check
```

自动化测试只使用内存凭据库、本地 HTTP server 和脱敏 fixture，不访问真实 DeepSeek/Kimi
账户或 Windows 凭据。真实契约测试须由维护者在本机显式提供临时低权限 Key，并在结束后删除。

## 工程文档

- [多供应商 Product Spec](docs/product/quotadock-product-spec-v2.md)
- [ADR-0001：签名更新与结构化额度数据源](docs/adr/0001-signed-updates-and-structured-usage.md)
- [ADR-0002：多供应商余额与凭据存储](docs/adr/0002-multi-provider-balances-and-credential-storage.md)
- [v0.6.0 发布说明](docs/releases/v0.6.0.md)
- [紧凑状态条设计系统](design-system/quotadock/MASTER.md)
- [多供应商实施计划](docs/superpowers/plans/2026-08-13-multi-provider-balance-plan.md)

更早版本的发布说明与历史设计资料保留在 `docs/`。当前产品边界以本 README、Product
Spec 与已接受 ADR 为准。
