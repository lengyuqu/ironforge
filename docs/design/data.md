# 数据、存储与附件

> 状态（0.5.1）：全部为已实现行为。事实来源：`crates/rg-db/`、`crates/rg-core/src/blob_storage/`、`crates/rg-core/src/repo/`、`crates/rg-http/src/api/`。
> 设计过程见 git 历史；这里只写当前契约。

## 1. 数据库后端

- 默认 SQLite；PostgreSQL / MySQL 通过 cargo feature（`db-postgres` / `db-mysql`）编入，按 `database_url` 的 scheme 选择。
- 迁移由 SeaORM 在启动时执行；同一套迁移支持三种后端。
- 全文检索按后端实现：SQLite FTS5、PostgreSQL `tsvector` + GIN、MySQL FULLTEXT；不降级为 `LIKE`。
- 不提供 SQLite → PostgreSQL/MySQL 的数据迁移工具；换后端只适用于新部署。
- 跨后端 SQL 注意：
  - MySQL 不支持 `ON CONFLICT DO NOTHING`；
  - MySQL 时间戳只存整秒，排序不能只靠 `(created_at, id)`；
  - SQLite 下「先读后写」的长事务在并发时可能返回 500，写路径要缩短事务或重试。

## 2. Blob 存储

`rg-core::blob_storage` 是二进制对象的统一边界，数据库只存 `BlobKey`（或可确定性派生的 key），本地磁盘只是当前 backend。

接口：`put`、`put_file`（流式）、`get`、`metadata`、`exists`、`delete`（幂等）、`list(prefix)`、可选的 `local_path`。

本地后端写入流程：目标目录内写随机临时文件 → `flush + sync_all` → rename。读者不会看到半成品。key 做长度、控制字符、绝对路径、空段、`.`/`..`、隐藏段、反斜杠校验；读写删除会 canonicalize 并拒绝越过 backend root 的 symlink。

对象键（用户可控段逐段 percent-encode）：

| 类型 | 键 |
|---|---|
| LFS | `lfs/{owner}/{repo}/{oid[0..2]}/{oid}.zst` |
| Package | `packages/{owner}/{repo}/{type}/{name}/{version}/{filename}` |
| OCI blob | `oci/{owner}/{repo}/blobs/sha256/{hash[0..2]}/{hash}` |
| OCI manifest | `oci/{owner}/{repo}/manifests/sha256/{hash}` |
| CI artifact | `artifacts/jobs/{job_id}/{uuid}-{name}` |
| Release asset | `releases/{owner}/{repo}/{release_id}/{asset_id}/{filename}` |
| 附件 | `attachments/{repo_id}/{uuid}/{filename}` |
| Archive cache（预留） | `archives/{repo_id}/{commit_sha}/{format}` |

一致性：没有分布式事务，采用补偿写入——先发布 Blob，再写 DB；DB 失败立即删 Blob。删除先删 Blob 再删 DB，DB 失败尝试恢复。`list` 与 DB 清单的差集用于发现 orphan 与 dangling。历史绝对路径仍可读，但**兼容读取不等于迁移完成**。

## 3. 附件

`attachments` 用四个外键表达归属：Issue、Pull Request、Issue Comment、Review Comment。每行带 `repo_id`，读取/下载/删除都校验「仓库 + 目标 + 附件」三层归属；跨仓或错配返回 404。

- 单文件 ≤100 MiB；仓库默认总配额 1 GiB；扩展名白名单；文件名 ≤255 字节；拒绝路径、控制字符和空文件。
- 上传与删除允许目标作者或仓库写者；列表与下载走仓库读权限。
- Issue 使用 Gitea 兼容的 `/repos/{owner}/{repo}/issues/{number}/assets` 与 `/issues/comments/{comment_id}/assets`；PR/Review Comment 使用 `/pulls/{number}/assets` 与 `/pulls/comments/{comment_id}/assets`。
- 上传先原子发布 Blob 再写 DB；删除时先临时备份 Blob，Blob 删成功再删 DB，DB 失败则恢复 Blob。

## 4. Markdown 模板

只从**默认分支**读取，不执行工作区文件、不跟随符号链接。

- Issue 模板目录顺序：`ISSUE_TEMPLATE` → `.gitea/ISSUE_TEMPLATE` → `.github/ISSUE_TEMPLATE` → `.gitlab/ISSUE_TEMPLATE`（大小写变体都认），只读目录直属 `*.md`。
- chooser 配置：`.gitea` 或 `.github` 模板目录下的 `config.yaml` / `config.yml`。
- PR 模板：根目录 → `.gitea` → `.github` 的 `PULL_REQUEST_TEMPLATE.md` / `pull_request_template.md`，取第一个匹配。
- 单文件 ≤1 MiB，必须 UTF-8；无效文件不进列表并记录日志。
- 支持 YAML front matter：`name`、`about`（兼容 `description`）、`title`、`labels`、`assignees`、`ref`；解析失败按普通 Markdown 处理。
- 接口：`/issue_templates`、`/issue_config`、`/issue_config/validate`、`/pull_request_template`（不存在返回 204）。

YAML Issue Form、模板继承、实例级默认模板、非默认分支模板都**未实现**。

## 5. 备份边界

完整恢复必需：数据库快照、Git 仓库目录、配置、密钥/版本清单，以及 durable Blob namespace（LFS、Package、OCI、Artifact、Release、附件）。

不进入必需集合：临时上传、`.tmp` 文件、CI Cache（可再生）。恢复顺序：Blob 对象 → 数据库 → Git 仓库 → 配置 → 一致性校验与协议抽查。

## 6. 不变量

1. 数据库只存 BlobKey；本地绝对路径不是对象身份。
2. 所有 key 经校验与编码；删除只接受已验证 key 或受 root 保护的 legacy 路径。
3. Blob 与 DB 之间用补偿而非分布式事务；失败要留下可发现、可修复的差集。
4. 附件与模板的跨仓库探测统一 404，不泄露存在性。
