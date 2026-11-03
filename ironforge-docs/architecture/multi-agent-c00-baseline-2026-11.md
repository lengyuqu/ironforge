# 同账号多 Agent 协作 C00 基线核验

> 2026-11-04；静态检查。基线 commit `7c9f07e26403c9c5a9fd4453195445895293c184`，分支 `main`，工作区存在既有未提交修改和未跟踪文件。不是测试或运行验证。

- 鉴权：`crates/rg-http/src/api/auth.rs` 的 `extract_user_id` 从 cookie/JWT 取得用户；`crates/rg-http/src/lib.rs` 的 PAT 路径将 PAT 解析后转为 JWT。实现协作接口须验证路由中间件 scope、JWT 撤销以及 cookie/bearer 优先级，不应直接假设 handler 能区分 PAT 主体或会话。
- 仓库权限：`crates/rg-http/src/api/repo_access.rs` 有认证读和写检查；`crates/rg-core/src/repo/service.rs` 权限缓存为进程内 30 秒，部分撤权在其他实例可能延迟。`IRONFORGE_DISABLE_PERM_CACHE=1` 可绕过缓存（首次使用时读取）。即时撤权需明确使用无缓存检查或将此限制列为验收未达成，不能仅复用缓存路径并声称即时生效。
- 数据库：SeaORM workspace 配置包含 SQLite、PostgreSQL、MySQL；`crates/rg-db/src/migrations/mod.rs` 注册迁移。CI 配有 PostgreSQL/MySQL smoke 服务（`.github/workflows/regression.yml`）。仅证明构建配置与 CI 定义存在，未连接真实三数据库验证事务语义。协作消息分配序号需要各后端真实并发测试。
- MCP：`crates/rg-mcp/src/tools/mod.rs` 为五个只读工具；`client.rs` 只有 GET；`lib.rs::AppState` 保存 API 地址与 PAT。新增会话及写工具需要扩展客户端、密钥生命周期与错误映射；保持 secret 不进入模型可见结果或日志。
- 并行工作区：现有改动涉及 `.ai/README.md`、HTTP AI/CI 路由和文档；新增未跟踪文件包括 CI 反馈、测试与设计文档。特别是 `crates/rg-http/src/api/mod.rs`、`lib.rs`、`openapi.rs` 已有他人或在制修改，后续只追加必要注册，不覆盖已有内容。
- 候选改动范围：`crates/rg-db/src/migrations/` 和注册表、`entities/`、`ops/`；`crates/rg-core/src/` 新增协作服务；`crates/rg-http/src/api/` 新增路由及 `lib.rs`、`openapi.rs` 注册；`crates/rg-mcp/src/` 客户端/工具；对应测试与接入文档。最终文件及是否需要修改 `AppState` 需实施时复核。

## 待验证

1. 三数据库真实服务可用性、迁移、锁和并发序号；当前未执行。不得用 SQLite 模拟其余后端通过。
2. PAT scope 中间件在协作路径的具体挂载与权限撤销实际行为；需集成测试。
3. 通知和审计敏感头的日志脱敏；需实际请求验证。
4. C01–C06 及 T01–T12 尚未实施或验收。后续设计、迁移及安全变更须遵循 `.ai/guardrails.md`。
