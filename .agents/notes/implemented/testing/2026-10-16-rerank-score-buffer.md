# Agent Note: 重排决策分数缓冲复用

Status: implemented

## Problem

`Reranker::best_where` 每次按键评分都会新建一个最多九个 `f32` 的结果 `Vec`。分数只用于当前调用选择要提升的候选，模型前向结束后不需要跨调用保留元素；反复申请这份短缓冲会落在输入热路径上。

## Decision

`Reranker` 持有 `scores_scratch`。`best_where` 取出备用向量、清空并按本轮可比候选数补足容量，完成比较后把向量放回；公开的 `log_probabilities` 仍返回独立拥有的分数向量，不改变 API 或调用方所有权。评分顺序、浮点运算、候选索引和提升判定保持不变。

## Existing decisions

[重排候选索引固定缓冲](2026-10-16-rerank-selected-indices.md)只消除筛选索引和引用数组；[重排候选状态缓冲跨轮复用](2026-10-16-rerank-resume-buffer.md)只复用 `resume` 外层状态。分数结果的生命周期独立于这两份状态，本片只处理 `best_where` 的短结果容器。

## Alternatives considered

- 每次继续 `Vec::with_capacity`：实现最简单，但每次重排都申请和释放同一份短结果存储。
- 让 `best_where` 直接返回分数向量：会把内部工作区所有权暴露给调用方，无法复用且扩大 API 契约。
- 固定数组承载分数：最多九项可以这样做，但公开的 `log_probabilities` 仍需要动态返回值；单独维护两套写入路径会增加重复逻辑，备用 `Vec` 可覆盖两种候选数量。

## Verification

新增 `best_where_reuses_the_score_buffer_capacity`，先执行一次评分建立容量，再执行第二次评分核对备用向量地址未变。参考实现 `cargo test --locked` 通过 27 个单元测试、14 个模型文件测试和 doc tests；全目标 Clippy（`-D warnings`）与 Rustfmt 通过。分数结果的逐位和排序契约由既有 `best_where_keeps_original_candidate_indices`、缓存复用和模型文件测试覆盖。

## Consequences

连续 `best_where` 调用复用最多九个 `f32` 的外层存储，减少一次短堆分配；公开分数查询仍按调用返回拥有型结果，模型前向、缓存状态、候选顺序和浮点结果不变。该片只减少容器分配，不能单独证明九键 p95 已达到 16 ms。
