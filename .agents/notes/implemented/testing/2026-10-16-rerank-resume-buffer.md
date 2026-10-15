# Agent Note: 重排候选状态缓冲跨轮复用

Status: implemented

## Problem

`Reranker::score_indices` 为每次评分新建 `next Vec`，用它保存本轮候选的编码和 `TailRun`，最后再替换上一轮的 `resume`。上一轮状态必须在当前评分期间继续存在以供完全命中和共享前缀匹配，因此旧实现无法直接把 `resume` 清空后复用其存储；但每次按键仍会为最多九个候选重新申请一个外层向量。

## Decision

`Reranker` 增加一个 `resume_scratch` 备用向量。评分开始时交换 `resume` 与备用向量：当前 `resume` 清空后承接本轮结果，交换出来的上一轮状态作为 `previous` 只读匹配；评分结束时清掉 `previous` 中的元素但保留容量，再放回备用向量。完全命中和最长共享前缀的遍历顺序分别保持“上一轮优先”和“本轮优先”。上下文前缀失效时清空 `previous`，避免跨前缀复用；备用向量清空后才保留，避免延长 `Arc<TailRun>` 的寿命。

## Alternatives considered

- 每轮继续 `Vec::with_capacity`：实现最简单，但候选列表的外层状态容器会在每次按键申请和释放。
- 直接清空 `resume` 后原地构造：不能同时保留上一轮状态，完全命中和共享前缀匹配会失去输入。
- 使用固定数组承载 `TailRun`：元素含有可变长度 token 向量和引用计数状态，固定数组需要额外的初始化或不安全代码；备用 `Vec` 只复用外层分配，不改变元素所有权。

## Verification

先加入 `reranker_reuses_the_resume_buffer_capacity`，实现前按预期因缺少 `resume_scratch` 字段编译失败。实现后参考实现 26 个单元测试、14 个模型文件测试和 doc tests、全目标 Clippy（`-D warnings`）、Rustfmt 与 `git diff --check` 均通过。既有完全命中、同批共享和缓存寿命测试覆盖上一轮状态仍可读且本轮结束后不会由备用缓冲延长引用寿命；新增测试还确认第三轮评分复用了备用向量的地址。

## Consequences

连续重排复用候选状态外层向量的容量，减少每轮评分的一次外层堆分配；候选 token 向量、`TailRun` 和评分结果的元素分配仍按各自生命周期工作。备用向量持有峰值容量但不持有元素，上一轮状态在本轮结束后按原规则释放。模型前向成本不变，主项目接入后仍需用固定资源验证九键端到端延迟。
