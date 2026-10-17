# Agent Note: 重排按候选索引评分并复用固定缓冲

Status: implemented

## Problem

`Reranker::best_where` 每次重排先把最多九个可比候选的索引收集到 `Vec<usize>`，再把这些索引对应的文本复制到临时 `Vec<&str>`，之后才进入模型评分。两个小数组的堆分配与模型前向无关，却位于九键输入的每次按键路径上。候选选择还必须保留原候选列表的索引，否则重排结果会无法交回引擎。

## Decision

`comparable` 使用带固定 `[usize; 9]` 存储的私有 `Compared`，只暴露有效索引切片。`best_where` 将这些原始索引直接传给评分循环；`score` 和 `best_where` 共用按索引迭代的 `score_indices`，按迭代器下界为结果缓冲预留容量。评分仍按原候选顺序产生，返回值仍是输入 `texts` 中的原始索引，最多比较九个候选，模型前向和分数公式不变。

## Alternatives considered

- 继续使用两个小 `Vec`：实现改动最小，但每次热路径调用都会保留与模型无关的临时堆分配。
- 复制候选文本而不是索引：可以保持评分接口简单，但仍然分配，并且不能直接表达最终需要的原始候选索引。
- 引入 `SmallVec`：可以把常见情况放在栈上，但要增加依赖和泛型边界；候选上限已是本模块的不变量，私有固定数组更直接。

## Verification

先加入 `comparable_indices_use_fixed_storage` 测试，未实现 `Compared` 时编译按预期失败；实现后该测试与所有参考实现测试通过。新增 `best_where_keeps_original_candidate_indices`，用合成模型把选中候选与独立评分结果逐项对照，覆盖跳过非可比候选以及返回原始索引。随后运行 `cargo test --manifest-path reference/Cargo.toml --locked`、全目标 Clippy（`-D warnings`）、Rustfmt 和 `git diff --check`。

## Consequences

每次重排不再为候选索引和候选引用建立临时堆数组，固定数组在 `Compared` 中占用少量栈空间；候选筛选、顺序、门控和模型结果保持不变。模型前向仍是主要成本，本切片不能单独证明九键 p95 已达到宿主的 16 ms 预算；主项目接入后仍需用固定资源重新测量。
