# QuotaDock

QuotaDock 是一个 Windows 优先的 Codex 额度状态工具。它以 `360 × 36px`
常驻悬浮条展示 5 小时与 1 周额度、重置时间、数据新鲜度和异常状态，并通过
系统托盘提供刷新、显示/隐藏、开机启动、更新检查和退出操作。

当前版本：`0.5.1`

## 当前能力

- 优先通过 Codex App Server 的结构化接口读取额度；PTY `/status` 仅作兼容降级。
- 显示 OpenAI 当前提供的额度窗口及动态重置倒计时；未提供的 5 小时窗口按可选能力处理。
- 区分新鲜、读取中、失败、陈旧、数据不完整、低额度和空数据状态。
- 查询失败时保留最后一次成功快照，并明确标记失败或陈旧。
- 后台自动刷新；低额度、临近重置和连续失败时自适应调整频率。
- 支持托盘手动刷新、单实例、窗口拖动吸附、位置保存和开机启动。
- 提供轻量详情页：数据来源、诊断、设置、恢复提示、账户摘要与 7 天趋势。
- 可选低额度系统通知；默认关闭，启用时明确请求 Windows 通知权限。
- 自动检查 GitHub Release 更新；下载和安装前使用应用内置公钥验证签名。

## 数据与隐私

QuotaDock 在 Tauri 应用数据目录的 `quotadock-state.json` 中保存最新成功快照、
最多 7 天的稀疏趋势采样、用户设置与待确认的恢复提示。原始 CLI 输出不会持久化；
应用不读取或保存 Codex 的认证、令牌内容。损坏或不兼容的状态文件会先备份，
仅保留最近 3 份恢复备份。

更新器只读取本项目 GitHub Release 的 `latest.json`，并以应用内置 Ed25519
公钥验证安装包签名。SHA-256 仍随发布资产提供，用于人工核验和旧版本兼容。
Windows 安装包当前未配置商业 Authenticode 代码签名证书，因此 SmartScreen
仍可能显示“未知发布者”；这与应用内更新签名是两条独立信任链。

## 开发

安装依赖：

```powershell
npm install
```

运行浏览器开发服务器：

```powershell
npm run dev
```

运行前端测试、类型检查和 Svelte 检查：

```powershell
npm test
```

运行桌面应用：

```powershell
npm run tauri dev
```

构建 Windows 安装包：

```powershell
npm run tauri build
```

Rust 全目标检查与测试：

```powershell
cargo check --manifest-path src-tauri/Cargo.toml --all-targets --all-features
cargo test --manifest-path src-tauri/Cargo.toml --all-features
```

Windows GNU 构建需要 Rust、Node.js、WebView2 与 MSYS2 UCRT64 工具链。测试目标
会显式嵌入 Common Controls v6 清单，避免 Windows 对话框入口点加载失败。

## 工程文档

- [产品工程审计与改进报告](docs/reviews/2026-07-30-product-engineering-audit.md)
- [Product Spec v2](docs/product/quotadock-product-spec-v2.md)
- [核心信任与数据源架构决策](docs/adr/0001-signed-updates-and-structured-usage.md)
- [v0.5.0 发布说明](docs/releases/v0.5.0.md)
- [v0.5.1 发布说明](docs/releases/v0.5.1.md)
- [紧凑状态条设计系统](design-system/quotadock/MASTER.md)
- [早期产品设计稿（历史资料）](docs/superpowers/specs/2026-06-18-codex-usage-tool-design.md)
- [早期 MVP 实施计划（历史资料）](docs/superpowers/plans/2026-06-18-quotadock-mvp.md)

早期设计稿描述的是粘贴 `/status` 与较完整仪表盘方案，已不代表当前实现；
后续产品决策应以本 README、工程审计报告及新的 Product Spec 为准。
