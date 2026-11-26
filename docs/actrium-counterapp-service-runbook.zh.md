# Actrium Counterapp Service 启动与 App 联调 Runbook

> 基于 2026-11-13 在 `verify/pr431-pr440` 分支上的实际联调整理。
> 当前 `main` 已合入 PR #445/#446/#451，目录结构与本文记录的示例路径可能不同；
> 如果 `packages/actrium/keel-guest-examples/counterapp` 不存在，请从对应发布分支或仓库取包。

## 0. 角色与链路

正式链路是「Realm owner 启动 Service，普通用户用 browser invite 在 App 调用」：

```
Front (Keel Guest, App 内渲染)
  -> host:service/<route> (Keel fixed-route)
  -> RealmServiceBinding (App 侧 Front actor)
  -> discover(serviceType) -> signaling route candidate
  -> callPeer(target: Service ActorId)
  -> Service 的 WASM workload
```

Actrium App **不启动、不注册 Service**；Service 由 Realm owner 单独运行。

## 1. 前置条件

- 分支包含 Realm Front/Service broker（PR #440 或已合入的 main）。
- `actr` CLI 可用。启动 WASM Service 时必须带 `wasm-engine` feature：
  - 已安装的 `actr` 若不带该 feature，会报 `package target requires the wasm-engine feature`。
  - 源码方式使用 `cargo run -p actr-cli --features wasm-engine -- ...`。
- 目标 Realm 的 Realm Profile 可访问：
  ```bash
  curl -s https://<domain>/realm/<realm-id> | jq .
  ```
  确认 `packages.front` / `packages.service` 的 type、href、sha256。
- Realm owner 的 `ri_...` invite（**owner invite**，不是 browser invite）。
- counterapp 的 Front 包和 Service 包：
  - `dist/actr-front-test-<version>.actr`
  - `dist/actr-service-test-<version>.actr`

## 2. 构建 counterapp 包

在 `counterapp/` 目录：

```bash
bun run build            # 生成 workload/guest.js + ui/capability-manifest.json
bun run build:package    # 打 Front 包（manifest.toml 必须与当前包版本一致）
```

Service 包按发布流程生成 `dist/actr-service-test-<version>.actr`。

打包前/后检查 Front 包的 linkage 与 capability manifest 是否一致：

```bash
unzip -p dist/actr-front-test-<version>.actr workload/guest.js | head -c 240
unzip -p dist/actr-front-test-<version>.actr ui/capability-manifest.json
```

要求：

- `guest.js` 第一行是 `// keel:v1-linkage {...}`；
- `links` 里的 fixed route（如 `host:service/echo.v1.EchoService.Echo`）与
  `hostModules.service.rpc` 中的 route 一致；
- 组件列表与 `ui/capability-manifest.json` 的 `components` 一致。

如果 Front 包是旧的（manifest 只有 `imports: ["host:service"]` 但 guest 已经是 fixed-route），
App 会报 `host:service/<hex>` 未注册的 Render Error。重新 build 并重新打包即可。

## 3. 准备 `realm-service.toml`

放在 `actr.toml` 所在目录（例如 counterapp 根目录）。模板：

```toml
edition = 1

[package]
path = "dist/actr-service-test-<version>.actr"

[signaling]
url = "wss://<provider>/signaling/ws"

[issuer_endpoint]
url = "https://<provider>/issuer"

[deployment]
realm = "<realm-id>@<domain>"
realm_client_key = "service-owner-client.json"

[discovery]
visible = true

[observability]
filter_level = "info"
tracing_enabled = false

[webrtc]
force_relay = false
stun_urls = ["stun:<provider>:3478"]
turn_urls = ["turn:<provider>:3478"]

[hyper]
data_dir = "/tmp/counterapp-service-hyper"

[[trust]]
kind = "static"
pubkey_b64 = "<publisher 的 Base64 Ed25519 package signing key>"

[[acl.rules]]
permission = "allow"
type = "<publisher>/<front-name>@<version>"
realm = "self"
```

字段说明：

- `[package] path`：Service 包路径，相对 `realm-service.toml` 目录。
- `[deployment] realm`：完整 Realm 名，必须与 Realm Profile 的 `realm.name` 一致。
- `realm_client_key`：owner client key 文件，相对配置文件目录。
- `[hyper] data_dir`：Service 的运行目录；不要用仓库目录，避免污染。
- `[[trust]]`：**`actr run` 读的是 `[[trust]]`，不是 `[hyper.trust]`**。
  `pubkey_b64` 是 publisher 的 package signing key（Publisher Profile 的 `keys[].public_key`）。
- `[[acl.rules]]`：允许 Front actor 类型调用 Service。`type` 必须与 Front 包
  `[actor]` 的 `publisher/name@version` 完全一致，`realm = "self"` 表示同 Realm。

## 4. 兑换 owner client key

在 `realm-service.toml` 所在目录：

```bash
actr realm join - --output service-owner-client.json
```

然后：

1. 粘贴完整的 `ri_...`
2. 按 **Return**
3. 按 **Ctrl+D** 结束输入

成功后输出类似：

```json
{
  "key_id": "...",
  "member_id": "...",
  "realm": "<realm-id>@<domain>",
  "role": "owner"
}
```

`role` 必须是 `owner`；`editor` 不能注册 Service。

如果本机已有该 Realm 的有效 owner key，直接放到同目录并设权限：

```bash
chmod 600 service-owner-client.json
```

注意：owner recovery 会撤销旧 owner client key，不要和已有 owner 共用。

## 5. 启动 Service

```bash
cd <realm-service.toml 所在目录>
actr run --config realm-service.toml --hyper-dir /tmp/counterapp-service-hyper
```

源码方式（必须显式开 wasm）：

```bash
cargo run -q --manifest-path <repo>/Cargo.toml -p actr-cli --features wasm-engine -- \
  run --config realm-service.toml --hyper-dir /tmp/counterapp-service-hyper
```

成功标志：

```
✅ ActrNode initialized
✅ Package attached
✅ Issuer credential bootstrap succeeded
✅ Connected to signaling server
🆔 Assigned ActrId: <actor-id>.<realm-id>@<domain>
✅ ActrNode started
📡 Running in foreground mode
```

保持终端运行。`Ctrl+C` 停止。

## 6. 验证 Service 收到请求

Service 日志（前台终端或重定向文件）出现：

```
rpc.mailbox.dequeued request_id=...
ActrNode.handle_incoming route_key=echo.v1.EchoService.Echo
wasm host dispatch entered: RpcEnvelope delivered into wasm guest
wasm guest dispatch entered: request ... payload_len=5
wasm guest response generated: ... response_len=11
response send attempted: routing guest response back to caller
response send succeeded via relay gate / direct RTC reliable lane
```

`payload_len=5` 通常是 `hello`，`response_len=11` 是 `echo: hello`。

## 7. 生成 App 用的 browser invite

Service 使用 owner key；App 使用单独的 browser invite（同 Realm、owner member 下的新 client）：

```bash
actr realm invite --key service-owner-client.json --member <owner-member-id>
```

复制返回的 `invite`（`ri_...`）。这个 invite 只给 Actrium App 兑换，不要拿去启动 Service。

## 8. App 端操作

1. 启动包含 Realm broker 的 Actrium（PR #440 或 main 合入后的构建）。
2. 地址栏输入：
   ```
   actr://<domain>/<realm-id>
   ```
3. 首次使用会弹 Redeem，粘贴 **browser invite**。
4. 点击 Create Instance。
5. 输入消息，点击 Send。
6. 期望聊天窗口显示 `echo: ...`。

## 9. 常见错误排查

| 错误 | 原因 / 处理 |
| --- | --- |
| `Public key not found for static trust anchor` | `actr run` 需要 `[[trust]]`，不是 `[hyper.trust]`；或包旁 `public-key.json` 不存在 |
| `package target requires the wasm-engine feature` | CLI 启动时没带 `--features wasm-engine`；App 端不需要本地启动 Service（PR #440+ 走 broker） |
| `service_owner_only` | `service-owner-client.json` 的 role 是 `editor`，需要 owner invite |
| `invite_used` / HTTP 409 | invite 一次性，已被使用；找 owner 重新发 |
| `No route candidates for type ...` | signaling 上没有 Service route candidate；先确认 Service 已 `ActrNode started`、signaling 已连接、`realm-service.toml` 的 Realm/provider 正确。ACL 在这一步之后才生效，不是原因 |
| Render Error `host:service/<hex>` not registered | Front 包的 capability manifest 与 guest linkage 不一致；重新 `bun run build` + 重新打包；如果走 `actr://`，还要更新 Realm Profile 指向的 Front 包 |
| `No route candidates` 但 Service 日志有 WebRTC 连接 | 检查 App 的 `discover(targetType:)` 是否拿到 ActorId；Service 重启后 ActorId 会变，Front 每次调用都会重新 discover |
| `Discarded message ... UDP:0.0.0.0:5353` / `unhandled STUN packet` | mDNS/ICE 的噪音，通常可忽略 |
| `WS lane ... closed` | WebRTC 连接重建；看是否随即出现新的 `DataLane` 和消息日志 |

## 10. 本次联调记录（2026-11-13）

- Realm：`actr-test-0-0-5@realm.develenv.com`
- Front/Service：`publisher.develenv.com/actr-front-test@0.0.5` / `actr-service-test@0.0.5`
- Service 启动后 ActorId：`38d29e1c22000.actr-test-0-0-5@realm.develenv.com`
- Service 日志：`/tmp/counterapp-service-005.log`
- Service 成功收到两次请求（`payload_len=5`），WASM Guest 返回 `response_len=11`，
  分别通过 `relay gate` 和 `direct RTC reliable lane` 回传。

## 11. 安全与清理

- `service-owner-client.json`、owner invite、browser invite 都不要提交到仓库或贴到公共渠道。
- Service 默认前台运行；停止用 `Ctrl+C` 或 `kill <pid>`。
- 临时目录 `/tmp/counterapp-service-hyper*` 可删除；`data_dir` 删除后 Service 会重建。
- 如果 `counterapp` 目录被移动或删除，旧进程的 cwd 会失效；先停止旧进程再重建。
