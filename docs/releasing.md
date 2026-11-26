# 发布流程

> 目标：版本、文档、CI 一次对齐。发布提交必须是文档检查的最后一个通过点。
> 自动门禁：`.github/workflows/regression.yml` 的 `Docs consistency` job，以及 `scripts/check-docs.py`。

## 0. 前置

- `main` 绿；工作区干净；GitHub 与 CNB 都可写。
- 版本粒度：0.x 阶段以 minor 为 breaking 粒度；修复走 patch。
- 已推送的标签不移动、不删除；发布出错就出下一个 patch。

## 1. 先同步文档

- 更新 `CLAUDE.md` 的状态行、未实现 / 技术债、依赖版本表。
- 行为有变化：先改对应的 `docs/design/*.md`，再写 CHANGELOG。
- 更新 `docs/operations.md`、`docs/performance.md` 与各设计文档的 `状态（X.Y.Z）`。
- 对外能力变化：同步 `README.md` 与 `README.zh-CN.md`（英文为主，章节结构一致）。
- 不新增带日期的快照、一次性核实、已完成任务记录；旧内容先合并，再删除。
- 跑 `python3 scripts/check-docs.py`。

检查内容：版本一致性、相对链接、`docs/README.md` 索引、文件名不带日期 / `verification`、无已删除路径引用。

## 2. 版本与 CHANGELOG

- 10 个 crate 的 `Cargo.toml`、`web/package.json`、`Cargo.lock` 同步到 X.Y.Z。
- `CHANGELOG.md` 顶部新增 `## [X.Y.Z] - YYYY-MM-DD`，首段写主线与关键数字。
- 校验清单与 lock 一致：

~~~sh
cargo metadata --offline --no-deps > /dev/null
python3 scripts/check-docs.py
~~~

## 3. 验证

~~~sh
scripts/check.sh --full
~~~

必要时再跑本机 smoke（前端、Git 协议、数据库）。文档检查在 `--quick` 与 `--full` 都会先跑。

## 4. 提交、标签、推送

~~~sh
git add -A
git commit -m "chore: release X.Y.Z"
git tag -a vX.Y.Z
git push github main
git -c credential.helper= -c credential.helper='!cnb git-credential' push cnb main
git push github vX.Y.Z
git -c credential.helper= -c credential.helper='!cnb git-credential' push cnb vX.Y.Z
~~~

标签消息写主线与关键数字，不复述 CHANGELOG。先推 main 再推 tag；两端都要推。

## 5. 发布后

- 两端 live ref 等于本地 SHA。
- GitHub Regression 的 `Docs consistency`、测试 job 与 Docker Image 都要绿。
- 汇报分层：本机命令、CI run id 与提交 SHA、远端 ref。推送成功不等于 CI 通过。

## 完成定义（DoD）

- [ ] crate / web / CHANGELOG / `CLAUDE.md` / `AGENT.md` / 设计状态行版本一致
- [ ] `docs/README.md` 索引了每个 docs 文件
- [ ] 相对链接 0 失效
- [ ] 无已删除路径引用、无日期 / `verification` 文件名
- [ ] README 中英版本存在、互相链接、`##` 章节数一致
- [ ] 两端 `main` 与 tag 已推送且 SHA 一致
- [ ] `Docs consistency` job 绿
