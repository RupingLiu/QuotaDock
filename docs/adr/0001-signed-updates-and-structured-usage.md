# ADR-0001：签名更新与结构化额度数据源

- 状态：Accepted
- 日期：2026-07-30
- 适用版本：QuotaDock 0.5.0+

## 背景

旧实现从 GitHub `latest.json` 同时取得下载地址和 SHA-256。它能发现传输损坏，
但不能在仓库发布权限失守时独立证明发布者身份。旧的“结构化降级”也仍把输出
交给文本解析器，无法形成稳定协议边界。

## 决策

1. 使用 Tauri updater 与内置 Ed25519 公钥。发布私钥只存于受控发布环境，
   安装包签名不匹配时拒绝下载后安装。
2. 发布清单使用 Tauri 的 `platforms.windows-x86_64.signature/url`，并额外保留
   `sha256/size/filename`，使 0.2.x 自定义更新器仍能升级到 0.5.0。
3. 主数据源改为 Codex App Server JSONL 协议：
   `initialize` → `initialized` → `account/rateLimits/read`。
4. `/status` PTY 文本解析只在结构化查询失败时启用；来源必须明确标记。
5. Authenticode 不作为应用内更新签名的替代。没有可信代码签名证书时，
   发布资料需披露 Windows 可能显示未知发布者。

## 结果

- 更新信任不再只依赖同一个远程清单。
- 数据字段由严格 JSON 反序列化与窗口时长映射产生，CLI UI 文案变化不再影响主链路。
- 仍保留旧 CLI 的可用性，但兼容解析器的风险不会伪装成结构化成功。
- 发布流程必须保护签名私钥并保存恢复方式；丢失私钥需要发布新应用公钥版本。

## 被否决方案

- 仅保留 SHA-256：无法独立验证发布者。
- 仅依赖 HTTPS/GitHub Release：仓库发布权限本身仍是单点信任。
- 删除 PTY：会立即放弃不支持 App Server 接口的旧 Codex CLI 用户。
- 把私钥提交到仓库：不可接受。
