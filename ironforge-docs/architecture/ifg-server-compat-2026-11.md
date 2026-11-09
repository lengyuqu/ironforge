# ifg 服务端兼容契约（开发分支）

- 仓库读取返回 `settings_version`。`PATCH /repos/{owner}/{name}` 需要管理员权限和 `expected_version`，支持 `description`（字符串或 null）、`is_private`、`default_branch`。版本基于设置内容，相同设置再次出现时版本相同；不一致返回 409。默认分支必须存在，更新时同步裸仓库 HEAD；发现 HEAD 与数据库不一致时拒绝更新。数据库提交失败时尽力恢复 HEAD。Git 和数据库并非同一事务，进程在更新 HEAD 与提交数据库之间退出时仍可能需要人工核对和修复。
- Wiki 读取和创建返回不透明 `version`；更新必须携带 `expected_version`，缺少字段返回 422，旧版本返回 409。修订记录和正文更新处于同一事务。旧客户端必须升级后才能编辑。客户端要保护读取到开始编辑之间的时间窗口，应保存读取时的版本并显式传给更新请求。
- 已认证的 `GET /api/v1/collaboration/capabilities` 除协作能力外，还返回构建提交 `build_commit`、是否有未提交改动 `build_dirty` 和 `api_capabilities`。未知的构建信息返回 null，不推测。`scripts/build-image-with-provenance.sh` 要求工作区干净，并从指定提交的 Git 归档构建，避免构建过程中工作区变化；其他构建方式须传入 `IRONFORGE_BUILD_COMMIT` 和 `IRONFORGE_BUILD_DIRTY` 构建参数，才能提供同等来源信息。提交号只标识基础提交；`build_dirty: true` 表示构建内容含未提交更改，不等同于该提交。
- 导出当前源码的 OpenAPI：`cargo run -q -p rg-http --example export_openapi > openapi.json`。ifg 的 `~ifg-compat` 快照来自开发源码，不能证明已部署实例包含这些接口。
